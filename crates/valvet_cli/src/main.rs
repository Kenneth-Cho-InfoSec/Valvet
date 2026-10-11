use std::path::{Path, PathBuf};
use std::process::Command;

use valvet_core::{codegen, pipeline};

const VERSION: &str = "0.2.0";

fn main() {
    let code = run(std::env::args().skip(1).collect());
    std::process::exit(code);
}

fn run(args: Vec<String>) -> i32 {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
        print_help();
        return 0;
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("valvet {}", VERSION);
        return 0;
    }
    match args[0].as_str() {
        "check" => cmd_check(get_path(&args)),
        "build" => cmd_build(get_path(&args), default_exe(get_path(&args))),
        "run" => cmd_run(get_path(&args)),
        "test" => cmd_test(args.get(1).map(|s| s.as_str())),
        "new" => cmd_new(args.get(1).map(|s| s.as_str())),
        _ => {
            eprintln!("error: unknown command `{}`", args[0]);
            eprintln!("run `valvet --help` for usage");
            2
        }
    }
}

fn get_path(args: &[String]) -> &str {
    if args.len() < 2 {
        eprintln!("error: missing <file.valvet>");
        eprintln!("run `valvet --help` for usage");
        std::process::exit(2);
    }
    &args[1]
}

fn print_help() {
    println!("valvet {} — Valvet language compiler (Core MVP)", VERSION);
    println!();
    println!("USAGE:");
    println!("    valvet check <file.valvet>   parse and analyse without linking");
    println!("    valvet build <file.valvet>   compile and link an executable");
    println!("    valvet run <file.valvet>     compile, link and execute");
    println!("    valvet test [dir]            run the Valvet fixture test suite");
    println!("    valvet new <name>            scaffold a new single-dir project");
    println!("    valvet --version             display version");
    println!("    valvet --help                show this help");
    println!();
    println!("EXIT CODES: 0 success, 1 compilation failure, 2 usage error.");
    println!("`run` exits with the compiled program's status on success.");
}

/// Print non-fatal warnings (if any) to stderr; compilation succeeded.
fn emit_warnings(checked: &valvet_core::sema::Checked) {
    use valvet_core::source::Source;
    let fallback = Source::new("<unknown>".to_string(), String::new());
    for w in &checked.warnings {
        let src = w
            .file
            .as_ref()
            .and_then(|f| checked.files.iter().find(|s| &s.name == f))
            .unwrap_or(checked.files.first().unwrap_or(&fallback));
        eprintln!("{}", w.render(src));
    }
}

fn cmd_check(path: &str) -> i32 {
    match pipeline::front_end_file(Path::new(path)) {
        Ok(checked) => {
            emit_warnings(&checked);
            println!("ok: {} is well-formed", path);
            0
        }
        Err(fd) => {
            eprintln!("{}", fd.render());
            1
        }
    }
}

/// Scaffold a minimal single-dir project (`valvet.toml`-less in 0.1.0: the
/// project root is the entry file's directory; `import a::b::x` loads
/// `<root>/a/b.valvet` declaring `module a.b;`).
fn cmd_new(name: Option<&str>) -> i32 {
    let name = match name {
        Some(n) => n,
        None => {
            eprintln!("error: missing <name> (`valvet new <name>`)");
            return 2;
        }
    };
    let dir = Path::new(name);
    if dir.exists() {
        eprintln!("error: `{}` already exists", name);
        return 1;
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: cannot create `{}`: {}", name, e);
        return 1;
    }
    let main = dir.join("main.valvet");
    let template = "fun main() {
    print(42)
}
";
    if let Err(e) = std::fs::write(&main, template) {
        eprintln!("error: cannot write `{}`: {}", main.display(), e);
        return 1;
    }
    println!("created {} (entry: {})", name, main.display());
    println!("hint: add `util.valvet` with `module util;` + `import util::help;` for multi-file");
    0
}

fn target_dir() -> PathBuf {
    PathBuf::from("target/valvet")
}

fn default_exe(path: &str) -> PathBuf {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    target_dir().join(stem)
}

/// Front-end + C lowering + cc link. Returns exe path or prints diags.
/// File-based: `path` is the entry file; its imports resolve from its directory.
fn build_exe(path: &str) -> Result<PathBuf, i32> {
    let checked = pipeline::front_end_file(Path::new(path)).map_err(|fd| {
        eprintln!("{}", fd.render());
        1
    })?;
    emit_warnings(&checked);
    let c_src = codegen::lower(&checked).map_err(|ice| {
        eprintln!("error[ICE]: internal compiler error: {}", ice.0);
        1
    })?;
    let dir = target_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("error: cannot create `{}`: {}", dir.display(), e);
        return Err(1);
    }
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    let c_file = dir.join(format!("{}.c", stem));
    let exe = dir.join(stem);
    if let Err(e) = std::fs::write(&c_file, &c_src) {
        eprintln!("error: cannot write `{}`: {}", c_file.display(), e);
        return Err(1);
    }
    if let Err(e) = codegen::cc_build(&c_file, &exe) {
        eprintln!("error: backend linking failed:\n{}", e);
        return Err(1);
    }
    Ok(exe)
}

fn cmd_build(path: &str, _exe: PathBuf) -> i32 {
    match build_exe(path) {
        Ok(exe) => {
            println!("built {}", exe.display());
            0
        }
        Err(c) => c,
    }
}

fn cmd_run(path: &str) -> i32 {
    let exe = match build_exe(path) {
        Ok(e) => e,
        Err(c) => return c,
    };
    match Command::new(&exe).status() {
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("error: cannot execute `{}`: {}", exe.display(), e);
            1
        }
    }
}

// ---------- fixture test suite ----------

struct Fixture {
    /// Entry `.valvet` file (flat fixture, or `main.valvet` of a multi-file dir).
    path: PathBuf,
    /// Stem for generated C/exe names (flat: file stem; multi-file: dir name).
    stem: String,
    kind: &'static str, // pass | fail | run
}

fn collect_fixtures(root: &Path) -> Vec<Fixture> {
    let mut out = vec![];
    for kind in [
        "compile-pass",
        "compile-fail",
        "runtime",
        "conformance",
        "regression",
    ] {
        let dir = root.join(kind);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("valvet") {
                let stem = p
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("t")
                    .to_string();
                out.push(Fixture {
                    path: p,
                    stem,
                    kind: if kind == "compile-fail" {
                        "fail"
                    } else {
                        "pass"
                    },
                });
            }
        }
    }
    // Multi-file projects: `multi-file/<name>/main.valvet` (+ siblings).
    // Fail kind when `main.valvet.error` exists, else pass.
    let multi = root.join("multi-file");
    if let Ok(entries) = std::fs::read_dir(&multi) {
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let entry = dir.join("main.valvet");
            if !entry.is_file() {
                continue;
            }
            let stem = dir
                .file_name()
                .and_then(|x| x.to_str())
                .unwrap_or("t")
                .to_string();
            let mut side = entry.as_os_str().to_owned();
            side.push(".error");
            let kind = if Path::new(&side).is_file() {
                "fail"
            } else {
                "pass"
            };
            out.push(Fixture {
                path: entry,
                stem,
                kind,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn sidecar(path: &Path, ext: &str) -> Option<String> {
    let mut p = path.as_os_str().to_owned();
    p.push(ext);
    std::fs::read_to_string(Path::new(&p))
        .ok()
        .map(|s| s.trim().to_string())
}

fn cmd_test(dir: Option<&str>) -> i32 {
    let root = PathBuf::from(dir.unwrap_or("valvet-tests"));
    let fixtures = collect_fixtures(&root);
    if fixtures.is_empty() {
        eprintln!("error: no fixtures found under {}", root.display());
        return 1;
    }
    let mut pass = 0u32;
    let mut fail = 0u32;
    for f in &fixtures {
        let name = f.path.display().to_string();
        if let Err(e) = std::fs::read_to_string(&f.path) {
            println!("FAIL {} (cannot read: {})", name, e);
            fail += 1;
            continue;
        }
        if f.kind == "fail" {
            let expected = sidecar(&f.path, ".error");
            match pipeline::front_end_file(&f.path) {
                Ok(_) => {
                    println!("FAIL {} (expected compile failure, compiled ok)", name);
                    fail += 1;
                }
                Err(fd) => {
                    let codes = fd.codes();
                    let ok = match &expected {
                        Some(e) => codes.iter().any(|c| *c == e),
                        None => true,
                    };
                    if ok {
                        println!("ok {} (rejected {:?})", name, codes);
                        pass += 1;
                    } else {
                        println!(
                            "FAIL {} (expected {}, got {:?})\n{}",
                            name,
                            expected.unwrap(),
                            codes,
                            fd.render()
                        );
                        fail += 1;
                    }
                }
            }
            continue;
        }
        // pass/run: build + execute + verify stdout/exit
        let checked = match pipeline::front_end_file(&f.path) {
            Ok(c) => c,
            Err(fd) => {
                println!("FAIL {} (unexpected compile error):\n{}", name, fd.render());
                fail += 1;
                continue;
            }
        };
        let c_src = match codegen::lower(&checked) {
            Ok(c) => c,
            Err(ice) => {
                println!("FAIL {} (ICE: {})", name, ice.0);
                fail += 1;
                continue;
            }
        };
        let dir = target_dir().join("fixtures");
        let _ = std::fs::create_dir_all(&dir);
        let stem = &f.stem;
        let c_file = dir.join(format!("{}.c", stem));
        let exe = dir.join(stem);
        if std::fs::write(&c_file, &c_src).is_err() {
            println!("FAIL {} (cannot write C)", name);
            fail += 1;
            continue;
        }
        if let Err(e) = codegen::cc_build(&c_file, &exe) {
            println!("FAIL {} (link failed: {})", name, e);
            fail += 1;
            continue;
        }
        let output = match Command::new(&exe).output() {
            Ok(o) => o,
            Err(e) => {
                println!("FAIL {} (cannot run: {})", name, e);
                fail += 1;
                continue;
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let code = output.status.code().unwrap_or(-1);
        let exp_out = sidecar(&f.path, ".stdout");
        let exp_exit: i32 = sidecar(&f.path, ".exit")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let ok = exp_out.as_deref().map(|e| e == stdout).unwrap_or(true) && code == exp_exit;
        if ok {
            println!("ok {}", name);
            pass += 1;
        } else {
            println!(
                "FAIL {} (expected stdout {:?} exit {}, got {:?} exit {})",
                name, exp_out, exp_exit, stdout, code
            );
            fail += 1;
        }
    }
    println!(
        "{} passed, {} failed ({} fixtures)",
        pass,
        fail,
        pass + fail
    );
    if fail > 0 {
        1
    } else {
        0
    }
}
