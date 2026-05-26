//! End-to-end tests driving the nanoc binary.

use std::path::Path;
use std::process::{Command, Output};

fn nanoc(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nanoc"))
        .args(["--no-color"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("nanoc runs")
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nanoc-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const GOOD: &str = "schema demo\n\nimport \"common.nb\"\n\nmessage Packet {\n  sequence: u32 @1\n  header: common.Header @2\n}\n";
const COMMON: &str = "schema common\n\nmessage Header {\n  source: u32 @1\n}\n";

#[test]
fn check_succeeds_on_valid_schema() {
    let dir = tempdir("check-ok");
    write(&dir, "demo.nb", GOOD);
    write(&dir, "common.nb", COMMON);
    let out = nanoc(&["check", "demo.nb"], &dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("2 file(s), 2 message(s)"), "{stdout}");
}

#[test]
fn check_fails_with_rendered_diagnostics() {
    let dir = tempdir("check-bad");
    write(
        &dir,
        "bad.nb",
        "schema demo\nmessage M {\n  x: u32 @1\n  y: u32 @1\n}\n",
    );
    let out = nanoc(&["check", "bad.nb"], &dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[E0109]"), "{stderr}");
    assert!(stderr.contains("field id @1 is used twice"), "{stderr}");
    assert!(stderr.contains("bad.nb:4"), "{stderr}");
}

#[test]
fn check_reports_missing_imports_with_location() {
    let dir = tempdir("check-missing-import");
    write(&dir, "demo.nb", GOOD);
    let out = nanoc(&["check", "demo.nb"], &dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error[E0300]"), "{stderr}");
    assert!(stderr.contains("imported here"), "{stderr}");
}

#[test]
fn build_writes_rust_code() {
    let dir = tempdir("build-rust");
    write(&dir, "demo.nb", GOOD);
    write(&dir, "common.nb", COMMON);
    let out = nanoc(
        &["build", "demo.nb", "--lang", "rust", "--out", "generated"],
        &dir,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let code = std::fs::read_to_string(dir.join("generated/demo.rs")).unwrap();
    assert!(code.contains("pub mod demo {"), "{code}");
    assert!(code.contains("pub mod common {"), "{code}");
    assert!(code.contains("pub struct Packet {"), "{code}");
}

#[test]
fn build_rejects_unknown_language() {
    let dir = tempdir("build-unknown-lang");
    write(&dir, "demo.nb", "schema demo\nmessage M {\n}\n");
    let out = nanoc(&["build", "demo.nb", "--lang", "cobol"], &dir);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown language `cobol`"));
}

#[test]
fn layout_prints_offsets_and_padding() {
    let dir = tempdir("layout");
    write(
        &dir,
        "demo.nb",
        "schema demo\nmessage Packet {\n  a: u8 @1\n  b: u64 @2\n  active: bool @3\n  t: f64? @4\n  name: string @5\n}\n",
    );
    let out = nanoc(&["layout", "demo.nb"], &dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("message demo.Packet"), "{stdout}");
    assert!(stdout.contains("bytes 8..16"), "{stdout}");
    assert!(stdout.contains("bit 3.0"), "{stdout}");
    assert!(stdout.contains("presence 3.1"), "{stdout}");
    // The string slot fills the 4..8 hole left by the u64's alignment.
    assert!(stdout.contains("slot 4"), "{stdout}");
}

#[test]
fn layout_includes_hints_on_stderr() {
    let dir = tempdir("layout-hints");
    write(
        &dir,
        "demo.nb",
        "schema demo\nmessage M {\n  a: u8 @1\n  b: u64 @2\n  c: u32 @3\n  d: u16 @4\n}\n",
    );
    let out = nanoc(&["layout", "demo.nb"], &dir);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("hint[H0001]"));
}

#[test]
fn migrate_converts_proto_files() {
    let dir = tempdir("migrate");
    write(
        &dir,
        "legacy.proto",
        "syntax = \"proto3\";\npackage demo;\nmessage Packet {\n  int32 raw = 1;\n  repeated string tags = 2;\n}\n",
    );
    let out = nanoc(&["migrate", "legacy.proto"], &dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let nb = std::fs::read_to_string(dir.join("legacy.nb")).unwrap();
    assert!(nb.contains("schema demo"), "{nb}");
    assert!(nb.contains("raw: i32 @1 [proto = int32]"), "{nb}");
    assert!(nb.contains("tags: list<string> @2"), "{nb}");

    // The migrated file passes check.
    let out = nanoc(&["check", "legacy.nb"], &dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn checking_a_proto_file_points_to_migrate() {
    let dir = tempdir("check-proto");
    write(&dir, "x.proto", "syntax = \"proto3\";\nmessage M {}\n");
    let out = nanoc(&["check", "x.proto"], &dir);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nanoc migrate"), "{stderr}");
}
