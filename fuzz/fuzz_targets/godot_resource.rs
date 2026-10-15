//! Godot scene/resource text, including nested values and section attributes.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|text: &str| {let _=bt_godot::validate_resource_text(text);});
