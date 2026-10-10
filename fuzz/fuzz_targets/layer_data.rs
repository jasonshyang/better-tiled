//! Layer data: the input's first byte picks an encoding and compression, the rest is the
//! payload of a 4×4 layer.
#![no_main]

use libfuzzer_sys::fuzz_target;

const ENCODINGS: [&str; 5] = [
    r#"encoding="csv""#,
    r#"encoding="base64""#,
    r#"encoding="base64" compression="zlib""#,
    r#"encoding="base64" compression="gzip""#,
    r#"encoding="base64" compression="zstd""#,
];

fuzz_target!(|input: &[u8]| {
    let Some((&choice, payload)) = input.split_first() else {
        return;
    };
    let encoding = ENCODINGS[usize::from(choice) % ENCODINGS.len()];
    let payload: String = String::from_utf8_lossy(payload).replace(['<', '&'], "");
    let text = format!(
        r#"<map version="1.10" orientation="orthogonal" width="4" height="4" tilewidth="8" tileheight="8"><tileset firstgid="1" name="t" tilewidth="8" tileheight="8" tilecount="16" columns="4"/><layer id="1" name="l" width="4" height="4"><data {encoding}>{payload}</data></layer></map>"#
    );
    let _ = bt_tmx::Loader::default().parse_map(&text, std::path::Path::new("/fuzz/map.tmx"));
});
