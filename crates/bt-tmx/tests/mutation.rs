//! Mutated files never make the readers panic: a seeded pass over small maps, tilesets, worlds
//! and projects in both syntaxes. `BT_FUZZ_SEED` sets the seed and `BT_FUZZ_CASES` the count;
//! a failure prints both and the text that failed.

#![cfg(test)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use bt_test_data::Scratch;
use bt_tmx::{Loader, parse_project};
use bt_utils::rng::Rng;

const MAP_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" tiledversion="1.12.2" orientation="staggered" renderorder="right-down" width="3" height="2" tilewidth="16" tileheight="16" infinite="0" staggeraxis="y" staggerindex="odd" nextlayerid="4" nextobjectid="3">
 <properties>
  <property name="c" type="class" propertytype="Spawn"><properties><property name="hp" type="int" value="5"/></properties></property>
  <property name="l" type="list"><item type="int" value="1"/></property>
 </properties>
 <tileset firstgid="1" name="t" tilewidth="16" tileheight="16" tilecount="4" columns="2">
  <image source="t.png" trans="ff00ff" width="32" height="32"/>
  <tile id="1" type="Wall" probability="0.5"><objectgroup draworder="index" id="2"><object id="1" x="1" y="2" width="3" height="4"/></objectgroup><animation><frame tileid="0" duration="100"/></animation></tile>
  <wangsets><wangset name="w" type="corner" tile="-1"><wangcolor name="a" color="#ff0000" tile="-1" probability="1"/><wangtile tileid="0" wangid="0,1,0,1,0,1,0,1"/></wangset></wangsets>
 </tileset>
 <layer id="1" name="g" width="3" height="2"><data encoding="csv">
1,2,3,
4,0,2147483649
</data></layer>
 <layer id="2" name="z" width="3" height="2"><data encoding="base64" compression="zlib">
   eJxjZGBgYGJgYGIEAAAkAAU=
  </data></layer>
 <group id="3" name="grp" opacity="0.5"><objectgroup id="4" name="o"><object id="2" gid="2" x="1.5" y="2" width="16" height="16" rotation="45"><text wrap="1">hi</text></object><object id="3"><polygon points="0,0 1,1 2,0"/></object></objectgroup></group>
</map>
"##;

const MAP_JSON: &str = r#"{"type":"map","version":"1.10","orientation":"hexagonal","renderorder":"right-down","width":2,"height":2,"tilewidth":16,"tileheight":16,"hexsidelength":8,"staggeraxis":"x","staggerindex":"even","infinite":true,"nextlayerid":3,"nextobjectid":2,"compressionlevel":-1,
 "properties":[{"name":"c","type":"class","propertytype":"Spawn","value":{"hp":5,"inner":{"x":1}}}],
 "tilesets":[{"firstgid":1,"name":"t","tilewidth":16,"tileheight":16,"tilecount":2,"columns":0,"tiles":[{"id":0,"image":"a.png","imagewidth":16,"imageheight":16},{"id":1,"image":"b.png"}]}],
 "layers":[{"type":"tilelayer","id":1,"name":"g","width":16,"height":16,"startx":0,"starty":0,"encoding":"base64","compression":"gzip","chunks":[{"x":0,"y":0,"width":2,"height":1,"data":"H4sIAAAAAAAAA2NkYGBgBAD5gnQnCAAAAA=="}],"opacity":1,"visible":true,"x":0,"y":0},
           {"type":"objectgroup","id":2,"name":"o","draworder":"topdown","objects":[{"id":1,"x":1,"y":2,"ellipse":true,"properties":[{"name":"p","type":"float","value":1.5}]}],"opacity":1,"visible":true,"x":0,"y":0}]}"#;

const TILESET_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" tiledversion="1.12.2" name="t" tilewidth="8" tileheight="8" tilecount="2" columns="0">
 <grid orientation="isometric" width="8" height="4"/>
 <terraintypes><terrain name="grass" tile="0"/></terraintypes>
 <tile id="0" terrain="0,0,,0"><image source="a.png" width="8" height="8"/></tile>
 <tile id="1"><image width="8" height="8" format="png"><data encoding="base64">iVBORw0KGgo=</data></image></tile>
</tileset>
"#;

const WORLD: &str = r#"{"maps":[{"fileName":"a.tmx","x":0,"y":0,"width":10,"height":10}],"patterns":[{"regexp":"m(\\d+)_(\\d+)\\.tmx","multiplierX":10}],"onlyShowAdjacentMaps":true,"type":"world"}"#;

const PROJECT: &str = r#"{"folders":["."],"propertyTypes":[{"id":1,"name":"Spawn","type":"class","useAs":["property"],"members":[{"name":"hp","type":"int","value":1},{"name":"self","type":"class","propertyType":"Spawn","value":{}}]},{"id":2,"name":"E","type":"enum","values":["a","b"],"valuesAsFlags":true}],"commands":[{"Name":"x","Enabled":true}]}"#;

/// Tokens that mean something to the readers.
const TOKENS: [&str; 24] = [
    "<",
    ">",
    "/>",
    "\"",
    "{",
    "}",
    "[",
    "]",
    ",",
    ":",
    "-1",
    "0",
    "4294967295",
    "99999999",
    "1e309",
    "NaN",
    "&amp;",
    "<tile gid=\"7\"/>",
    "<chunk x=\"-5\" y=\"0\" width=\"9999\" height=\"9999\">",
    "\"data\":[1,2]",
    "\"chunks\":[{}]",
    "type=\"list\"",
    "\"type\":\"group\"",
    "\u{fffe}",
];

/// Values that mean something to the finishing code, for mutations that keep files well-formed.
const VALUES: [&str; 26] = [
    "",
    "-1",
    "0",
    "1",
    "4294967295",
    "99999999",
    "1e309",
    "-2147483648",
    "nan",
    "abc",
    "#zzzzzz",
    "0x10",
    "3.5",
    "true",
    ",,,",
    "../x.tsx",
    ":/automap-tiles.tsx",
    "base64",
    "zstd",
    "csv",
    "isometric",
    "hexagonal",
    "index",
    "class",
    "list",
    "0,1,0,1,0,1,0,9",
];

/// The text with the contents of some quoted values replaced: still well-formed, so the
/// mutation reaches the code that turns values into the model.
fn with_values_replaced(rng: &mut Rng, seed: &str) -> String {
    let mut text = seed.to_owned();
    for _ in 0..1 + rng.below(4).unwrap_or(0) {
        let quotes: Vec<usize> = text.match_indices('"').map(|(i, _)| i).collect();
        if quotes.len() < 2 {
            break;
        }
        let pair = usize::try_from(
            rng.below(u64::try_from(quotes.len() / 2).unwrap())
                .unwrap_or(0),
        )
        .unwrap();
        let (open, close) = (quotes[pair * 2], quotes[pair * 2 + 1]);
        let value = VALUES[usize::try_from(rng.below(VALUES.len() as u64).unwrap_or(0)).unwrap()];
        text.replace_range(open + 1..close, value);
    }
    text
}

fn mutated(rng: &mut Rng, seed: &str) -> String {
    if rng.below(2) == Some(0) {
        return with_values_replaced(rng, seed);
    }
    let mut chars: Vec<char> = seed.chars().collect();
    let mutations = 1 + rng.below(8).unwrap_or(0);
    for _ in 0..mutations {
        let len = u64::try_from(chars.len()).unwrap();
        let at = usize::try_from(rng.below(len + 1).unwrap_or(0)).unwrap();
        match rng.below(4).unwrap_or(0) {
            0 => {
                if at < chars.len() {
                    chars[at] =
                        char::from(b' ' + u8::try_from(rng.below(95).unwrap_or(0)).unwrap());
                }
            }
            1 => {
                let end =
                    (at + usize::try_from(rng.below(40).unwrap_or(0)).unwrap()).min(chars.len());
                chars.drain(at..end);
            }
            2 => {
                let token =
                    TOKENS[usize::try_from(rng.below(TOKENS.len() as u64).unwrap_or(0)).unwrap()];
                for (i, c) in token.chars().enumerate() {
                    chars.insert(at + i, c);
                }
            }
            _ => {
                let end =
                    (at + usize::try_from(rng.below(60).unwrap_or(0)).unwrap()).min(chars.len());
                let copy: Vec<char> = chars[at..end].to_vec();
                for (i, c) in copy.into_iter().enumerate() {
                    chars.insert(at + i, c);
                }
            }
        }
    }
    chars.into_iter().collect()
}

#[test]
fn mutated_files_never_panic_the_readers() {
    let seed: u64 = std::env::var("BT_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let cases: usize = std::env::var("BT_FUZZ_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let scratch = Scratch::new("bt-tmx-mutation").unwrap();
    let mut rng = Rng::new(seed);
    let seeds: [(&str, &str); 5] = [
        (MAP_XML, "map.tmx"),
        (MAP_JSON, "map.tmj"),
        (TILESET_XML, "tiles.tsx"),
        (WORLD, "w.world"),
        (PROJECT, "p.tiled-project"),
    ];
    let mut parsed = 0usize;
    for case in 0..cases {
        let (original, name) = seeds[case % seeds.len()];
        let text = mutated(&mut rng, original);
        let path = scratch.join(name);
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut loader = Loader::default();
            let path: &Path = &path;
            match name {
                "map.tmx" | "map.tmj" => loader.parse_map(&text, path).is_ok(),
                "tiles.tsx" => loader.parse_tileset(&text, path).is_ok(),
                "w.world" => loader.parse_world(&text, path).is_ok(),
                _ => parse_project(&text, path).is_ok(),
            }
        }));
        match outcome {
            Ok(read) => parsed += usize::from(read),
            Err(_) => panic!(
                "BT_FUZZ_SEED={seed} BT_FUZZ_CASES={cases}: case {case} ({name}) panicked on:\n{text}"
            ),
        }
    }
    // Enough mutated files are still well-formed for the pass to reach past the parsers.
    println!("{parsed} of {cases} mutated files read");
    assert!(
        parsed * 5 >= cases,
        "only {parsed} of {cases} mutated files read"
    );
}
