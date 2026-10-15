use serde_json::Value;
use std::path::Path;
pub(super) fn run_godot(godot: &Path, dir: &Path, args: &[&str]) -> (String, String) {
    let output = std::process::Command::new(godot)
        .args(["--headless", "--path"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}
pub(super) fn json_between_markers(stdout: &str, _stderr: &str) -> Value {
    let json = stdout
        .split("BT-JSON-BEGIN")
        .nth(1)
        .and_then(|s| s.split("BT-JSON-END").next())
        .unwrap_or_else(|| panic!("no JSON: {stdout}"));
    serde_json::from_str(json).unwrap()
}
