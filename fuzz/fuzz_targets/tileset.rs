//! Tilesets in either syntax.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = bt_tmx::Loader::default().parse_tileset(text, std::path::Path::new("/fuzz/tiles.tsx"));
});
