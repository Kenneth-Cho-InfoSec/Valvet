use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_valvet")
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "valvet-security-{}-{}-{}",
        tag,
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn target_triple_is_not_a_shell_fragment() {
    let dir = temp_dir("target");
    let entry = write(&dir, "main.valvet", "fun main() { println(1) }\n");
    let marker = dir.join("pwned");
    let triple = format!("x; touch {} #", marker.display());

    let out = run(&["check", "--target", &triple, entry.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "invalid target must fail before running a driver: {}",
        stderr(&out)
    );
    assert!(!marker.exists(), "shell metacharacters must never execute");
    assert!(stderr(&out).contains("invalid target triple"));
}

#[test]
fn hermetic_denies_file_io_and_cwd() {
    let dir = temp_dir("hermetic");
    let cases = [
        (
            "read_file",
            "fun main() { val r = read_file(\"/etc/hostname\"); println(r) }\n",
            "`read_file` is denied",
        ),
        (
            "read_file_limit",
            "fun main() { val r = read_file_limit(\"/etc/hostname\", 10); println(r) }\n",
            "`read_file_limit` is denied",
        ),
        (
            "write_file",
            "fun main() { val r = write_file(\"/tmp/valvet-denied\", \"x\"); println(r) }\n",
            "`write_file` is denied",
        ),
        (
            "sys_cwd",
            "fun main() { println(sys_cwd()) }\n",
            "`sys_cwd` is denied",
        ),
    ];
    for (name, body, needle) in cases {
        let entry = write(&dir, &format!("{name}.valvet"), body);
        let out = run(&["check", "--hermetic", entry.to_str().unwrap()]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{name} must be rejected under --hermetic: {}",
            stderr(&out)
        );
        assert!(
            stderr(&out).contains(needle),
            "{name} diagnostic should mention `{needle}`: {}",
            stderr(&out)
        );
    }

    let entry = write(
        &dir,
        "read_file_allowed.valvet",
        "fun main() { val r = read_file(\"/etc/hostname\"); println(r) }\n",
    );
    let out = run(&["check", entry.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "read_file must still work without --hermetic: {}",
        stderr(&out)
    );
}

#[test]
fn stdlib_shadowing_requires_explicit_opt_in() {
    let dir = temp_dir("stdlib");
    fs::create_dir_all(dir.join("std")).unwrap();
    write(
        &dir,
        "std/math.valvet",
        "module std.math;\npub fun shadow_marker(): Int64 { 7 as Int64 }\n",
    );
    let entry = write(
        &dir,
        "main.valvet",
        "import std.math::shadow_marker;\nfun main() { println(shadow_marker()) }\n",
    );

    let out = run(&["check", entry.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "local std/ must not shadow the pinned stdlib by default: {}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("unknown name `shadow_marker`"));

    let out = run(&["check", "--allow-stdlib-shadow", entry.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "explicit opt-in must allow the local std/ tree: {}",
        stderr(&out)
    );

    let out = run(&[
        "check",
        "--hermetic",
        "--allow-stdlib-shadow",
        entry.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "hermetic must ignore the stdlib-shadow opt-in: {}",
        stderr(&out)
    );
}

#[test]
fn deep_nesting_is_a_diagnostic_not_a_signal() {
    let dir = temp_dir("depth");
    let text = format!(
        "fun main() {{ println({}1{}); }}\n",
        "(".repeat(2000),
        ")".repeat(2000)
    );
    let entry = write(&dir, "deep.valvet", &text);
    let out = run(&["check", entry.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "parser depth limit must exit normally, not abort: {}",
        stderr(&out)
    );
    assert!(stderr(&out).contains("E0006"));
}

#[test]
fn read_file_limit_and_bytes_to_string_are_bounded() {
    let dir = temp_dir("bounded");
    let data = dir.join("data.bin");
    fs::write(&data, b"hello").unwrap();
    let body = format!(
        "fun main() {{\n         match (read_file_limit(\"{}\", 5 as Int64)) {{ Ok(s) -> println(s.len()), Err(e) -> println(e) }};\n         match (read_file_limit(\"{}\", 4 as Int64)) {{ Ok(s) -> println(\"unexpected\"), Err(e) -> println(e) }};\n         match (read_file_limit(\"{}\", 0 as Int64 - 1 as Int64)) {{ Ok(s) -> println(\"unexpected\"), Err(e) -> println(e) }};\n         match (bytes_to_string(vec![65 as Int64, 0 as Int64, 66 as Int64])) {{ Ok(s) -> println(s.len()), Err(e) -> println(e) }};\n         match (bytes_to_string(vec![256 as Int64])) {{ Ok(s) -> println(\"unexpected\"), Err(e) -> println(e) }};\n         }}\n",
        data.display(),
        data.display(),
        data.display()
    );
    let entry = write(&dir, "bounded.valvet", &body);
    let out = run(&["run", entry.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "run failed: {}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim(),
        "5\nfile exceeds max bytes\nnegative max bytes\n3\nbyte out of range"
    );
}
