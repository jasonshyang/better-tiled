//! Projects and worlds.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = bt_tmx::parse_project(text, std::path::Path::new("/fuzz/p.tiled-project"));
    let _ = bt_tmx::Loader::default().parse_world(text, std::path::Path::new("/fuzz/w.world"));
});
