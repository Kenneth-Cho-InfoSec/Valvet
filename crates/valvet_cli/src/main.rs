use std::path::{Path, PathBuf};
use std::process::Command;

use valvet_core::{codegen, pipeline};

const VERSION: &str = "0.4.0";

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
        "build" => cmd_build(
            get_path(&args),
            default_exe(get_path(&args)),
            args.iter().any(|a| a == "--frozen"),
        ),
        "run" => {
            if args.iter().any(|a| a == "--watch") {
                let f = args[1..]
                    .iter()
                    .find(|a| *a != "--watch")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                if f.is_empty() {
                    eprintln!("error: missing <file>");
                    return 2;
                }
                cmd_watch(f)
            } else {
                let frozen = args[2..].iter().any(|a| a == "--frozen");
                let clean: Vec<String> = std::iter::once(args[0].clone())
                    .chain(args[1..].iter().filter(|a| *a != "--frozen").cloned())
                    .collect();
                cmd_run(get_path(&clean), script_argv(&clean), frozen)
            }
        }
        "fmt" => cmd_fmt(get_path(&args)),
        "tree" => cmd_tree(get_path(&args)),
        "metadata" => cmd_metadata(get_path(&args)),
        "verify" => cmd_verify(get_path(&args)),
        "test" => cmd_test(args.get(1).map(|s| s.as_str())),
        "new" => cmd_new(args.get(1).map(|s| s.as_str())),
        "repl" => cmd_repl(),
        "-e" => cmd_eval(args.get(1).map(|s| s.as_str())),
        _ => {
            eprintln!("error: unknown command `{}`", args[0]);
            eprintln!("run `valvet --help` for usage");
            2
        }
    }
}

fn get_path(args: &[String]) -> &str {
    if args.len() < 2 {
        eprintln!("error: missing <file>");
        eprintln!("run `valvet --help` for usage");
        std::process::exit(2);
    }
    &args[1]
}

fn print_help() {
    println!("valvet {} — Valvet language compiler (Core MVP)", VERSION);
    println!();
    println!("USAGE:");
    println!("    valvet check <file>        parse and analyse without linking");
    println!("    valvet build <file>        compile and link an executable");
    println!("    valvet build <file> --frozen  verify against valvet.lock");
    println!("    valvet run <file>          compile, link and execute");
    println!("    valvet run <file> --frozen   frozen verify, then run");
    println!("    (.valvet, .vv and .vvt sources are equivalent)");
    println!("    valvet run <file> -- args    run with script arguments in `args`");
    println!("    valvet test [dir]            run the Valvet fixture test suite");
    println!("    valvet new <name>            scaffold a new single-dir project");
    println!("    valvet run --watch <file>    re-run on file change");
    println!("    valvet fmt <file>            normalize whitespace and trailing newline");
    println!("    valvet tree <file>           show the import dependency tree");
    println!("    valvet metadata <file>      print crate metadata as JSON");
    println!("    valvet verify <file>         recompute lock and cache digests");
    println!("    valvet repl                  interactive session (definitions persist)");
    println!("    valvet -e 'expr'              evaluate a scriptlet and print its value");
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
    let template = "mode exp;
fun main() {
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
/// Successful builds cache by content hash (version + toolchain + sources);
/// a cache hit skips C emission and linking entirely.
fn build_exe(path: &str, quiet: bool, frozen: bool) -> Result<PathBuf, i32> {
    let checked = pipeline::front_end_file(Path::new(path)).map_err(|fd| {
        if !quiet {
            eprintln!("{}", fd.render());
        }
        1
    })?;
    if !quiet {
        emit_warnings(&checked);
        // Reproducibility lock (Batch 4): sources plus toolchain, beside
        // the entry. Quiet builds (REPL sessions, `-e`) skip it; the test
        // harness never reaches here (it calls the front end directly).
        if frozen {
            if let Err(e) = check_lock(path, &checked) {
                eprintln!("error: frozen lock mismatch: {}", e);
                return Err(2);
            }
        } else if let Err(e) = write_lock(path, &checked) {
            eprintln!("error: cannot write lock: {}", e);
            return Err(1);
        }
    }
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    let dir = target_dir();
    let exe = dir.join(stem);
    let digest = exe_digest(path, &checked);
    let sidecar = sidecar_for(path, &dir, stem);
    // Sidecar holds the exe digest plus the lowered-C digest (early
    // cutoff, à la carte: identical C skips the link even when inputs
    // moved, e.g. comment-only edits or compiler rebuilds).
    let cached: Vec<String> = std::fs::read_to_string(&sidecar)
        .ok()
        .map(|s| s.lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default();
    if exe.exists() && cached.first().is_some_and(|d| *d == digest) {
        return Ok(exe);
    }
    let c_src = codegen::lower(&checked).map_err(|ice| {
        if !quiet {
            eprintln!("error[ICE]: internal compiler error: {}", ice.0);
        }
        1
    })?;
    let c_hex = valvet_core::sha256::hex(c_src.as_bytes());
    if exe.exists() && cached.get(1).is_some_and(|c| *c == c_hex) {
        if !quiet {
            println!("note: unchanged lowering; skipping link");
        }
        let _ = std::fs::write(&sidecar, format!("{}\n{}\n", digest, c_hex));
        return Ok(exe);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        if !quiet {
            eprintln!("error: cannot create `{}`: {}", dir.display(), e);
        }
        return Err(1);
    }
    let c_file = dir.join(format!("{}.c", stem));
    if let Err(e) = std::fs::write(&c_file, &c_src) {
        if !quiet {
            eprintln!("error: cannot write `{}`: {}", c_file.display(), e);
        }
        return Err(1);
    }
    if let Err(e) = codegen::cc_build(&c_file, &exe) {
        if !quiet {
            eprintln!("error: backend linking failed:\n{}", e);
        }
        return Err(1);
    }
    // Memoize only successful builds (a failed link must rebuild next time).
    let _ = std::fs::write(&sidecar, format!("{}\n{}\n", digest, c_hex));
    Ok(exe)
}

/// Recompute every recorded digest (Nix-style verification, no daemon).
fn cmd_verify(path: &str) -> i32 {
    let checked = match pipeline::front_end_file(Path::new(path)) {
        Ok(c) => c,
        Err(fd) => {
            eprintln!("{}", fd.render());
            return 1;
        }
    };
    if let Err(e) = check_lock(path, &checked) {
        println!("drift: {}", e);
        return 1;
    }
    println!("lock ok");
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    let sidecar = sidecar_for(path, &target_dir(), stem);
    let digest = exe_digest(path, &checked);
    match std::fs::read_to_string(&sidecar) {
        Ok(s) if s.lines().next().is_some_and(|d| d.trim() == digest) => {
            println!("cache ok");
            0
        }
        Ok(_) => {
            println!("drift: cached exe digest differs (rebuild to refresh)");
            1
        }
        Err(_) => {
            println!("drift: no cache entry (build first)");
            1
        }
    }
}

/// Cache sidecar for one entry stem (filename disambiguator only).
fn sidecar_for(path: &str, dir: &Path, stem: &str) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut ph = DefaultHasher::new();
    path.hash(&mut ph);
    dir.join(format!("{}-{:x}.hash", stem, ph.finish()))
}

/// `valvet.lock` beside the entry directory (empty parent = cwd).
/// Canonical build identity (Bazel lesson: every input that can move
/// the output participates): version, C driver, compiler build, sources.
fn exe_digest(path: &str, checked: &valvet_core::sema::Checked) -> String {
    let mut buf = vec![];
    buf.extend_from_slice(path.as_bytes());
    buf.push(0);
    buf.extend_from_slice(VERSION.as_bytes());
    buf.push(0);
    buf.extend_from_slice(std::env::var("VALVET_CC").unwrap_or_default().as_bytes());
    buf.push(0);
    // The compiler itself is an input: rebuilt binaries must not
    // serve pre-fix executables for unchanged sources.
    if let Ok(md) = std::env::current_exe()
        .and_then(|p| std::fs::metadata(&p))
        .and_then(|m| m.modified())
    {
        buf.extend_from_slice(format!("{:?}", md).as_bytes());
    }
    buf.push(0);
    let mut files: Vec<(&str, &str)> = checked
        .files
        .iter()
        .map(|f| (f.name.as_str(), f.text.as_str()))
        .collect();
    files.sort();
    for (n, t) in files {
        buf.extend_from_slice(n.as_bytes());
        buf.push(0);
        buf.extend_from_slice(t.as_bytes());
        buf.push(0);
    }
    for (n, v) in &checked.dep_locks {
        buf.extend_from_slice(n.as_bytes());
        buf.push(0);
        buf.extend_from_slice(v.as_bytes());
        buf.push(0);
    }
    valvet_core::sha256::hex(&buf)
}

fn lock_path_for(entry: &str) -> PathBuf {
    let mut d = Path::new(entry)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    if d.as_os_str().is_empty() {
        d = PathBuf::from(".");
    }
    d.join("valvet.lock")
}

fn file_hex(text: &str) -> String {
    valvet_core::sha256::hex(text.as_bytes())
}

/// Write the source lock after a successful front end.
fn write_lock(entry: &str, checked: &valvet_core::sema::Checked) -> std::io::Result<()> {
    let mut out = format!(
        "# generated by `valvet build`; do not edit\nvalvet {}\n",
        VERSION
    );
    for f in &checked.files {
        out.push_str(&format!("file {} {}\n", f.name, file_hex(&f.text)));
    }
    let mut deps = checked.dep_locks.clone();
    deps.sort();
    for (name, version) in &deps {
        out.push_str(&format!("dep {} {}\n", name, version));
    }
    std::fs::write(lock_path_for(entry), out)
}

/// Verify the working tree against an existing lock.
fn check_lock(entry: &str, checked: &valvet_core::sema::Checked) -> Result<(), String> {
    let body = std::fs::read_to_string(lock_path_for(entry))
        .map_err(|_| "no valvet.lock found".to_string())?;
    let mut want: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut version = String::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        match parts.next() {
            Some("valvet") => version = parts.next().unwrap_or_default().to_string(),
            Some("file") => {
                let name = parts.next().unwrap_or_default().to_string();
                let hash = parts.next().unwrap_or_default().to_string();
                want.insert(name, hash);
            }
            Some("dep") => {
                let name = parts.next().unwrap_or_default().to_string();
                let version = parts.next().unwrap_or_default().to_string();
                want.insert(format!("dep:{}", name), version);
            }
            _ => {}
        }
    }
    if version != VERSION {
        return Err(format!("lock wants valvet {}, have {}", version, VERSION));
    }
    for f in &checked.files {
        match want.get(&f.name) {
            Some(h) if *h == file_hex(&f.text) => {}
            Some(_) => return Err(format!("source changed: {}", f.name)),
            None => return Err(format!("source not locked: {}", f.name)),
        }
    }
    for (name, version) in &checked.dep_locks {
        match want.get(&format!("dep:{}", name)) {
            Some(v) if v == version => {}
            Some(_) => return Err(format!("dependency changed: {}", name)),
            None => return Err(format!("dependency not locked: {}", name)),
        }
    }
    Ok(())
}

fn cmd_build(path: &str, _exe: PathBuf, frozen: bool) -> i32 {
    match build_exe(path, false, frozen) {
        Ok(exe) => {
            println!("built {}", exe.display());
            0
        }
        Err(c) => c,
    }
}

fn cmd_watch(path: &str) -> i32 {
    use std::time::{Duration, SystemTime};
    let p = PathBuf::from(path);
    let mut last: Option<SystemTime> = None;
    loop {
        let cur = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
        if cur != last {
            last = cur;
            let code = cmd_run(path, vec![], false);
            if code != 0 {
                eprintln!("watch: exit {}", code);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn cmd_fmt(path: &str) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read `{}`: {}", path, e);
            return 1;
        }
    };
    let mut out = String::new();
    for line in src.lines() {
        out.push_str(line.trim_end().replace('\t', "    ").as_str());
        out.push('\n');
    }
    match std::fs::write(path, out) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("error: cannot write `{}`: {}", path, e);
            1
        }
    }
}

fn cmd_run(path: &str, script_argv: Vec<String>, frozen: bool) -> i32 {
    let exe = match build_exe(path, false, frozen) {
        Ok(e) => e,
        Err(c) => return c,
    };
    match Command::new(&exe).args(&script_argv).status() {
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("error: cannot execute `{}`: {}", exe.display(), e);
            1
        }
    }
}

/// Script arguments after `--` (`valvet run f.valvet -- a b` become the
/// script's `args`). Bare trailing flags are rejected to reserve the space.
fn script_argv(args: &[String]) -> Vec<String> {
    let rest = if args.len() > 2 { &args[2..] } else { &[] };
    if rest.first().is_some_and(|a| a == "--") {
        return rest[1..].to_vec();
    }
    if rest.iter().any(|a| a.starts_with('-') && *a != "--watch") {
        eprintln!("error: script arguments need `--` first (`valvet run f.valvet -- args...`)");
        std::process::exit(2);
    }
    rest.to_vec()
}

/// Evaluate a scriptlet: `valvet -e 'expr'` wraps it as `println(expr);`
/// (anything with `;`/braces runs as statements instead).
fn cmd_eval(expr: Option<&str>) -> i32 {
    let expr = match expr {
        Some(e) => e,
        None => {
            eprintln!("error: missing expression (`valvet -e 'expr'`)");
            return 2;
        }
    };
    let looks_like_stmts =
        expr.contains(';') || expr.contains('{') || expr.contains('}') || expr.contains('\n');
    let src = if looks_like_stmts {
        format!("{}\n", expr)
    } else {
        format!("println(({}));\n", expr)
    };
    let dir = target_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("error: cannot create `{}`: {}", dir.display(), e);
        return 1;
    }
    let path = dir.join("eval.valvet");
    if std::fs::write(&path, src).is_err() {
        eprintln!("error: cannot write `{}`", path.display());
        return 1;
    }
    cmd_run(&path.to_string_lossy(), vec![], false)
}

// ---------- fixture test suite ----------

struct Fixture {
    /// Entry file (flat fixture, or `main.{valvet,vv,vvt}` of a multi-file dir).
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
            if p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| ["valvet", "vv", "vvt"].contains(&e))
            {
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
    // Multi-file projects: `multi-file/<name>/main.{valvet,vv,vvt}`.
    // Fail kind when `main.*.error` exists, else pass.
    let multi = root.join("multi-file");
    if let Ok(entries) = std::fs::read_dir(&multi) {
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            let entry = ["main.valvet", "main.vv", "main.vvt"]
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.is_file());
            let entry = match entry {
                Some(p) => p,
                None => continue,
            };
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

/// Dependency tree from recorded import edges (cycle-safe).
fn cmd_tree(path: &str) -> i32 {
    let checked = match pipeline::front_end_file(Path::new(path)) {
        Ok(c) => c,
        Err(fd) => {
            eprintln!("{}", fd.render());
            return 1;
        }
    };
    let mut kids: std::collections::HashMap<&str, Vec<&str>> = std::collections::HashMap::new();
    for (a, b) in &checked.edges {
        kids.entry(a.as_str()).or_default().push(b.as_str());
    }
    for v in kids.values_mut() {
        v.sort();
    }
    fn show(
        kids: &std::collections::HashMap<&str, Vec<&str>>,
        node: &str,
        depth: usize,
        seen: &mut Vec<String>,
    ) {
        println!("{}{}", "  ".repeat(depth), node);
        if seen.contains(&node.to_string()) {
            return;
        }
        seen.push(node.to_string());
        if let Some(next) = kids.get(node) {
            for c in next {
                show(kids, c, depth + 1, seen);
            }
        }
        seen.pop();
    }
    let root = if checked.edges.is_empty() {
        path.to_string()
    } else {
        checked
            .edges
            .iter()
            .map(|(a, _)| a.clone())
            .find(|a| !checked.edges.iter().any(|(_, b)| b == a))
            .unwrap_or_else(|| path.to_string())
    };
    let mut seen = vec![];
    show(&kids, &root, 0, &mut seen);
    0
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Machine-readable crate metadata (format version 1).
fn cmd_metadata(path: &str) -> i32 {
    let checked = match pipeline::front_end_file(Path::new(path)) {
        Ok(c) => c,
        Err(fd) => {
            eprintln!("{}", fd.render());
            return 1;
        }
    };
    let mut out = String::from("{\"format_version\":1,");
    out.push_str(&format!("\"entry\":{},", json_str(path)));
    out.push_str("\"files\":[");
    let mut files: Vec<(&str, usize)> = checked
        .files
        .iter()
        .map(|f| (f.name.as_str(), f.text.len()))
        .collect();
    files.sort();
    for (i, (n, len)) in files.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{{\"path\":{},\"bytes\":{}}}", json_str(n), len));
    }
    out.push_str("],\"deps\":[");
    let mut deps = checked.dep_locks.clone();
    deps.sort();
    for (i, (n, v)) in deps.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"version\":{}}}",
            json_str(n),
            json_str(v)
        ));
    }
    out.push_str("],\"edges\":[");
    let mut edges = checked.edges.clone();
    edges.sort();
    for (i, (a, b)) in edges.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"from\":{},\"to\":{}}}",
            json_str(a),
            json_str(b)
        ));
    }
    out.push_str("]}");
    println!("{}", out);
    0
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

// ---------- interactive session (J7) ----------
//
// Model: definitions (`fun`/`struct`/`enum`) persist across entries and
// recompile every round; statement/expression entries are wrapped in a
// fresh `__rN` function and only the latest runs (no replay). `val`/`var`
// entries evaluate once and are discarded (later entries cannot see them).
// Errors never drop the session: failed entries roll back.
// Line editing is plain stdin (no rustyline: keeps builds hermetic/offline).

/// One persisted definition, sliced from its entry text by span.
struct ReplItem {
    kind: &'static str,
    name: String,
    text: String,
}

struct ReplSession {
    imports: Vec<String>,
    items: Vec<ReplItem>,
    counter: u32,
}

impl ReplSession {
    /// Session source: imports, definitions, latest snippet driver.
    /// `snippet` is `Some(entry)` to run, `None` for definitions-only.
    fn render(&self, snippet: Option<&str>, echo: bool) -> String {
        let mut s = String::new();
        for imp in &self.imports {
            s.push_str(imp);
            s.push('\n');
        }
        for it in &self.items {
            s.push_str(&it.text);
            s.push('\n');
        }
        match snippet {
            Some(entry) => {
                // Echo form prints bare-expression values (`1 + 2` shows
                // `3`); anything else runs as written.
                let body = if echo {
                    format!("println(({}));\n0\n", entry)
                } else {
                    format!("{}\n0\n", entry)
                };
                s.push_str(&format!(
                    "fun __r{}(): Int32 {{\n{}}}\n",
                    self.counter, body
                ));
                s.push_str(&format!("fun main() {{\n__r{}();\n}}\n", self.counter));
            }
            None => {
                s.push_str("fun main() {\n}\n");
            }
        }
        s
    }
}

/// Brace depth over one line, aware of strings/comments (heuristic for
/// continuation prompts; miscounts only on exotic nesting).
fn scan_line(
    line: &str,
    depth: &mut i32,
    in_block: &mut i32,
    in_str: &mut bool,
    triple: &mut bool,
) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if *in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                if *triple {
                    if i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                        *in_str = false;
                        *triple = false;
                        i += 3;
                        continue;
                    }
                } else {
                    *in_str = false;
                }
            }
            i += 1;
            continue;
        }
        if *in_block > 0 {
            if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                *in_block += 1;
                i += 2;
                continue;
            }
            if c == '*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                *in_block -= 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break;
        }
        if c == '/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            *in_block = 1;
            i += 2;
            continue;
        }
        if c == '"' {
            if i + 2 < bytes.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                *in_str = true;
                *triple = true;
                i += 3;
                continue;
            }
            *in_str = true;
            *triple = false;
            i += 1;
            continue;
        }
        if c == '{' {
            *depth += 1;
        } else if c == '}' {
            *depth -= 1;
        }
        i += 1;
    }
}

/// Slice `[start, end)` bytes from entry text (span-stamped, same buffer).
fn slice_of(entry: &str, start: usize, end: usize) -> String {
    entry.get(start..end).unwrap_or("").trim().to_string()
}

fn cmd_repl() -> i32 {
    use std::io::{BufRead, IsTerminal, Write};
    use valvet_core::source::Source;

    let interactive = std::io::stdin().is_terminal();
    if interactive {
        println!("valvet repl — definitions persist, snippets run once. `:help` for commands.");
    }
    let dir = target_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("error: cannot create `{}`: {}", dir.display(), e);
        return 1;
    }
    let session_path = dir.join("repl_session.valvet");
    let session_file = session_path.to_string_lossy().to_string();

    let mut session = ReplSession {
        imports: vec![],
        items: vec![],
        counter: 0,
    };
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut depth = 0i32;
    let mut in_block = 0i32;
    let mut in_str = false;
    let mut triple = false;
    let mut buf = String::new();
    let mut first_line = true;
    loop {
        if (first_line || depth != 0 || in_block != 0 || in_str) && interactive {
            print!("{}", if first_line { "valvet> " } else { "... " });
            let _ = std::io::stdout().flush();
        }
        let line = match lines.next() {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                eprintln!("error: cannot read stdin: {}", e);
                return 1;
            }
            None => break,
        };
        if !buf.is_empty() {
            buf.push('\n');
        }
        // Explicit continuation: a trailing `\` joins the next line
        // (for multi-item entries, e.g. a struct plus its functions).
        let continued = line.ends_with('\\') && !line.ends_with("\\\\");
        let piece = if continued {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        buf.push_str(piece);
        scan_line(piece, &mut depth, &mut in_block, &mut in_str, &mut triple);
        first_line = false;
        if continued || depth > 0 || in_block != 0 || in_str {
            continue;
        }
        // Complete entry.
        first_line = true;
        depth = 0;
        let entry = std::mem::take(&mut buf);
        let trimmed = entry.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ":quit" || trimmed == ":q" {
            break;
        }
        if trimmed == ":reset" {
            session = ReplSession {
                imports: vec![],
                items: vec![],
                counter: 0,
            };
            if interactive {
                println!("session reset");
            }
            continue;
        }
        if trimmed == ":help" {
            println!("entries: `fun`/`struct`/`enum` persist (redefine by name); anything else runs once.");
            println!("imports persist (`import a::b::x;`). `fun main` and `module` are rejected.");
            println!("commands: :help :reset :quit");
            continue;
        }
        if trimmed.starts_with(':') {
            eprintln!(
                "error: unknown command `{}` (`:help` lists commands)",
                trimmed
            );
            continue;
        }
        // Classify: items (persist) vs snippet (run once).
        let probe = Source::new("<repl>".to_string(), entry.clone());
        let as_unit = pipeline::parse_unit(&probe).ok();
        let has_items = as_unit
            .as_ref()
            .map(|p| !p.funs.is_empty() || !p.structs.is_empty() || !p.enums.is_empty())
            .unwrap_or(false);
        let has_module = as_unit.as_ref().and_then(|p| p.module.clone());
        if has_module.is_some() {
            eprintln!("error: `module` declarations live in files, not the repl");
            continue;
        }
        if has_items {
            let prog = as_unit.unwrap();
            if prog.funs.iter().any(|f| f.name == "main") {
                eprintln!("error: the repl drives `main` itself; define other functions");
                continue;
            }
            if !prog.imports.is_empty() {
                eprintln!("error: one thing per entry (imports go on their own line)");
                continue;
            }
            // Tentatively replace same-named definitions, roll back on failure.
            let saved_len = session.items.len();
            let saved: Vec<(String, String, String)> = session
                .items
                .iter()
                .map(|it| (it.kind.to_string(), it.name.clone(), it.text.clone()))
                .collect();
            for f in &prog.funs {
                session
                    .items
                    .retain(|it| !(it.kind == "fun" && it.name == f.name));
                session.items.push(ReplItem {
                    kind: "fun",
                    name: f.name.clone(),
                    text: slice_of(&entry, f.span.start, f.span.end),
                });
            }
            for d in &prog.structs {
                session
                    .items
                    .retain(|it| !(it.kind == "struct" && it.name == d.name));
                session.items.push(ReplItem {
                    kind: "struct",
                    name: d.name.clone(),
                    text: slice_of(&entry, d.span.start, d.span.end),
                });
            }
            for d in &prog.enums {
                session
                    .items
                    .retain(|it| !(it.kind == "enum" && it.name == d.name));
                session.items.push(ReplItem {
                    kind: "enum",
                    name: d.name.clone(),
                    text: slice_of(&entry, d.span.start, d.span.end),
                });
            }
            let text = session.render(None, false);
            if std::fs::write(&session_path, &text).is_err() {
                eprintln!("error: cannot write session file");
                session.items.truncate(saved_len);
                continue;
            }
            match pipeline::front_end_file(&session_path) {
                Ok(_) => {
                    if interactive {
                        println!("ok");
                    }
                }
                Err(fd) => {
                    eprintln!("{}", fd.render());
                    session.items.clear();
                    for (kind, name, text) in saved {
                        let kind = match kind.as_str() {
                            "fun" => "fun",
                            "struct" => "struct",
                            _ => "enum",
                        };
                        session.items.push(ReplItem { kind, name, text });
                    }
                }
            }
            let _ = std::fs::remove_file(&session_path);
            continue;
        }
        // Import-only entry: persist verbatim (deduplicated).
        if let Some(prog) = as_unit {
            if !prog.imports.is_empty() {
                if !session.imports.contains(&trimmed) {
                    session.imports.push(trimmed.clone());
                }
                let text = session.render(None, false);
                if std::fs::write(&session_path, &text).is_err() {
                    eprintln!("error: cannot write session file");
                    session.imports.retain(|i| i != &trimmed);
                    continue;
                }
                match pipeline::front_end_file(&session_path) {
                    Ok(_) => {
                        if interactive {
                            println!("ok");
                        }
                    }
                    Err(fd) => {
                        eprintln!("{}", fd.render());
                        session.imports.retain(|i| i != &trimmed);
                    }
                }
                let _ = std::fs::remove_file(&session_path);
                continue;
            }
        }
        // Snippet entry: must parse wrapped in a function body.
        let wrapped = format!("fun __rpl() {{\n{}\n}}\n", entry);
        let wprobe = Source::new("<repl>".to_string(), wrapped);
        if let Err(ds) = pipeline::parse_unit(&wprobe) {
            // Item-looking input gets unit errors, else expression errors.
            let first = entry.trim_start();
            let itemy = [
                "fun", "struct", "enum", "import", "module", "pub", "impl", "trait", "type",
                "const", "static",
            ]
            .iter()
            .any(|k: &&str| first.starts_with(*k));
            if itemy {
                match pipeline::parse_unit(&probe) {
                    Ok(_) => eprintln!("error: entry defines nothing usable"),
                    Err(es) => {
                        for d in es {
                            eprintln!("{}", d.render(&probe));
                        }
                    }
                }
            } else {
                for d in ds {
                    eprintln!("{}", d.render(&wprobe));
                }
            }
            continue;
        }
        // Check + run (failed snippets never persist: nothing was added).
        // Bare expressions echo their value; when the echo form does not
        // compile (e.g. Unit-valued calls), retry as a plain snippet.
        session.counter += 1;
        let echo = pipeline::is_bare_expr(&entry);
        let text = session.render(Some(&entry), echo);
        if std::fs::write(&session_path, &text).is_err() {
            eprintln!("error: cannot write session file");
            continue;
        }
        let exe = match build_exe(&session_file, true, false) {
            Ok(e) => e,
            Err(_) if echo => {
                let text = session.render(Some(&entry), false);
                if std::fs::write(&session_path, &text).is_err() {
                    eprintln!("error: cannot write session file");
                    continue;
                }
                match build_exe(&session_file, false, false) {
                    Ok(e) => e,
                    Err(_) => {
                        let _ = std::fs::remove_file(&session_path);
                        continue;
                    }
                }
            }
            Err(_) => {
                let _ = std::fs::remove_file(&session_path);
                continue;
            }
        };
        match Command::new(&exe).status() {
            Ok(st) => {
                if !st.success() {
                    eprintln!("exit {}", st.code().unwrap_or(1));
                }
            }
            Err(e) => eprintln!("error: cannot execute: {}", e),
        }
        let _ = std::fs::remove_file(&session_path);
    }
    0
}
