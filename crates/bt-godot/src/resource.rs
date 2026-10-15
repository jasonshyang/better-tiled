//! Validating Godot text resources without starting Godot.
/// Checks the syntax of a Godot text scene or resource, including nested values.
/// This is a syntax check; `verify` also resolves resources through Godot.
pub fn validate_resource_text(text: &str) -> Result<(), String> {
    crate::read::parse(text).map(|_| ())
}

#[cfg(test)]
mod tests {
    #[test]
    fn seeded_reader_mutations_never_panic() {
        let fuzz = bt_test_data::Fuzz::from_env(0x676f646f74, 1000);
        let source=b"[gd_scene format=3]\n[node name=\"room\" type=\"Node2D\"]\nmetadata/data = {\"a\": [Vector2(1,2), Color(1,0,0,1)]}\n";
        for case in 0..fuzz.cases {
            let mut bytes = source.to_vec();
            let seed = fuzz.case_seed(case);
            let at = (seed as usize) % bytes.len();
            bytes[at] = (seed >> 16) as u8;
            let text = String::from_utf8_lossy(&bytes);
            let _ = super::validate_resource_text(&text);
        }
    }
}
