//! XML maps: any text, read as a map next to nothing.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = bt_tmx::Loader::default().parse_map(text, std::path::Path::new("/fuzz/map.tmx"));
});
