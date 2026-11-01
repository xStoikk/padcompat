use std::fs;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_padcompat"))
}

#[test]
fn builtin_catalog_validates_without_reading_hardware() {
    let output = bin()
        .args(["profiles", "validate"])
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout, "profiles/controllers.toml: 3 profiles\n");
    assert!(!stdout.contains("serial"));
    assert!(!stdout.contains("kernel:"));
}

#[test]
fn invalid_catalog_exits_nonzero_and_names_the_file() {
    let path = std::env::temp_dir().join(format!(
        "padcompat-invalid-catalog-{}.toml",
        std::process::id()
    ));
    fs::write(
        &path,
        r#"
version = 1
[[controller]]
id = "left"
name = "Left"
vendor_id = 0x10
product_id = 0x20
expected_driver = "drv-a"
form = "wired"
[[controller.interface]]
class = 0xff
subclass = 1
protocol = 2

[[controller]]
id = "right"
name = "Right"
vendor_id = 0x10
product_id = 0x20
expected_driver = "drv-b"
form = "wired"
[[controller.interface]]
class = 0xff
subclass = 1
protocol = 2
"#,
    )
    .unwrap();
    let output = bin()
        .args(["profiles", "validate", "--profiles"])
        .arg(&path)
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&path);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
    assert!(
        stderr.contains("left") && stderr.contains("right"),
        "{stderr}"
    );
    assert!(stderr.contains("interface"), "{stderr}");
    assert!(!stderr.contains("serial"));
}

#[test]
fn malformed_toml_and_a_valid_replacement_file() {
    let bad = std::env::temp_dir().join(format!("padcompat-bad-toml-{}.toml", std::process::id()));
    fs::write(&bad, "version = [\n").unwrap();
    let output = bin()
        .args(["profiles", "validate", "--profiles"])
        .arg(&bad)
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&bad);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("profile toml"), "{stderr}");

    let good = std::env::temp_dir().join(format!(
        "padcompat-good-catalog-{}.toml",
        std::process::id()
    ));
    fs::write(
        &good,
        r#"
version = 1
[[controller]]
id = "fixture-synthetic-pad"
name = "Fixture Synthetic Pad"
vendor_id = 0x1234
product_id = 0xabcd
expected_driver = "fixture-driver"
form = "wired"
[[controller.interface]]
class = 0xff
subclass = 0x47
protocol = 0xd0
"#,
    )
    .unwrap();
    let output = bin()
        .args(["profiles", "validate", "--profiles"])
        .arg(&good)
        .output()
        .expect("spawn");
    let _ = fs::remove_file(&good);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("1 profiles"), "{stdout}");
    assert!(!stdout.contains("serial"));
}
