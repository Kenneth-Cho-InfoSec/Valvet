use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use crate::ast::Program;
use crate::diag::Diagnostic;
use crate::source::Source;

/// Module-qualified item name: entry items keep plain `name`, others are
/// `module::name` (`"a.b::Point"`). Qualified names are globally unique by
/// construction (one file per module path), so joint checking, the C backend
/// (after ident sanitizing), and diagnostics can all key on them.
pub fn qname(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.to_string()
    } else {
        format!("{}::{}", module, name)
    }
}

/// File-based module loader (Phase H): `import a::b::Item` in the entry file
/// loads `<root>/a/b.valvet` (root = entry file's directory), which must
/// declare `module a.b;`. Units are parsed separately (per-file errors) and
/// checked jointly by sema ("Env per module": each module sees its own items
/// plus its imports; cross-module access requires `pub`).
///
/// Rules (D9 §1 + VR-002):
/// - `module a.b;` (dots) declares a file's module; mismatch → E0701.
/// - imports use `::` only (dotted imports are E0602, at parse time).
/// - cycles → E0601; missing files → E0701; private leaks → E0401 (in sema).
/// - diamonds (two modules importing one) load once and are fine.
pub struct Unit {
    /// Display path as given (for diagnostics).
    pub display: String,
    /// Owning module (`""` for the entry).
    pub module: String,
    pub source: Source,
    pub program: Program,
}

pub struct Crate {
    /// Load order; `units[0]` is always the entry.
    pub units: Vec<Unit>,
    /// Every successfully-read source (superset of units: includes files
    /// that failed to parse, so their errors still render with position).
    pub sources: Vec<Source>,
}

impl Crate {
    /// Joint program for sema: entry items first, then dependencies in load
    /// order. Single C TU is emitted downstream (no separate compilation).
    pub fn joint(&self) -> Program {
        let mut funs = vec![];
        let mut structs = vec![];
        let mut enums = vec![];
        let mut imports = vec![];
        // Joint span is the entry's (entry-first diagnostics like E0603
        // attribute there).
        let span = self
            .units
            .first()
            .map(|u| u.program.span)
            .unwrap_or(crate::source::Span::empty(0));
        for u in &self.units {
            funs.extend(u.program.funs.iter().cloned());
            structs.extend(u.program.structs.iter().cloned());
            enums.extend(u.program.enums.iter().cloned());
            imports.extend(u.program.imports.iter().cloned());
        }
        Program {
            module: None,
            imports,
            funs,
            structs,
            enums,
            // Script synthesis already ran per unit in `load_unit`
            // (entry `top` became `main`; anything else errored).
            top: vec![],
            mode: crate::ast::Mode::Auto,
            span,
        }
    }

    /// Joint program with globally-unique ids (see `renumber`).
    pub fn joint_renumbered(&self) -> Program {
        let mut prog = self.joint();
        renumber(&mut prog);
        prog
    }
}

/// Ordered stdlib roots for `std::` imports (J0), first hit wins:
/// explicit `VALVET_STDLIB` override, install layout beside the binary,
/// then a live dev checkout. Missing candidates are skipped, so any
/// subset may exist.
pub fn default_stdlib_roots() -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Ok(v) = std::env::var("VALVET_STDLIB") {
        roots.push(PathBuf::from(v));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.join("../share/valvet/stdlib"));
        }
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib"));
    roots.into_iter().filter(|p| p.is_dir()).collect()
}

/// Load the entry file plus its transitive imports. Always returns the
/// (possibly partial) crate alongside errors so failures still render
/// per-file; callers proceed only when errors are empty.
/// Errors carry `.file` attribution for per-file rendering.
pub fn load(entry: &Path) -> (Crate, Vec<Diagnostic>) {
    let root = entry
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut l = Loader {
        root,
        stdlib_roots: default_stdlib_roots(),
        units: vec![],
        sources: vec![],
        index: HashMap::new(),
        stack: vec![],
        errs: vec![],
    };
    let entry_display = entry.display().to_string();
    let _ = l.load_unit(entry, &entry_display, String::new(), None);
    (
        Crate {
            units: l.units,
            sources: l.sources,
        },
        l.errs,
    )
}

struct Loader {
    root: PathBuf,
    /// Ordered fallbacks for `std::` imports (J0): explicit override,
    /// install layout, then a live dev checkout.
    stdlib_roots: Vec<PathBuf>,
    units: Vec<Unit>,
    sources: Vec<Source>,
    /// Normalized absolute path -> unit index (diamond dedup).
    index: HashMap<PathBuf, usize>,
    /// Load stack (normalized path + display) for cycle detection.
    stack: Vec<(PathBuf, String)>,
    errs: Vec<Diagnostic>,
}

impl Loader {
    /// Read, parse, and (for imports) verify one file, then recurse into its
    /// imports. `expected` is the declared module for imports (`None` entry).
    /// `via` is the triggering import's (file, span) for error attribution.
    fn load_unit(
        &mut self,
        path: &Path,
        display: &str,
        expected: String,
        via: Option<(&str, crate::source::Span)>,
    ) -> Result<(), ()> {
        let norm = normalize(path);
        // Cycle: target already on the load stack (displays as-given).
        if self.stack.iter().any(|(p, _)| p == &norm) {
            let chain: Vec<String> = self
                .stack
                .iter()
                .map(|(_, d)| d.clone())
                .chain(std::iter::once(display.to_string()))
                .collect();
            let (f, sp) = via.unwrap_or((display, crate::source::Span::empty(0)));
            self.errs.push(
                Diagnostic::error(
                    "E0601",
                    sp,
                    format!("circular import ({})", chain.join(" -> ")),
                )
                .with_file(f),
            );
            return Err(());
        }
        // Diamond: already loaded off-stack — reuse, no error.
        if self.index.contains_key(&norm) {
            return Ok(());
        }
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => {
                let (f, sp) = via.unwrap_or((display, crate::source::Span::empty(0)));
                let what = match via {
                    Some(_) => format!("cannot load imported file `{}`", display),
                    None => format!("cannot read `{}`", display),
                };
                self.errs.push(
                    Diagnostic::error("E0701", sp, format!("{} (unresolved module)", what))
                        .with_file(f),
                );
                return Err(());
            }
        };
        let source = Source::new(display.to_string(), text);
        self.sources.push(source.clone());
        let mut program = match crate::pipeline::parse_unit(&source) {
            Ok(p) => p,
            Err(ds) => {
                for d in ds {
                    self.errs.push(d.with_file(display));
                }
                return Err(());
            }
        };
        // Non-entry files must declare the module their path implies.
        let module = if expected.is_empty() {
            String::new()
        } else {
            match &program.module {
                Some(m) if *m == expected => expected.clone(),
                _ => {
                    self.errs.push(
                        Diagnostic::error(
                            "E0701",
                            program.span,
                            format!(
                                "file `{}` must declare `module {};` (file–module mismatch)",
                                display, expected
                            ),
                        )
                        .with_file(display),
                    );
                    return Err(());
                }
            }
        };
        // Script mode is entry-only. Imported files hold items; a mode
        // header or top-level statements there is E0604. The entry's
        // top-level statements become a synthesized `fun main` (dummy
        // ids — `renumber` reassigns every id before sema).
        if !expected.is_empty() {
            if program.mode != crate::ast::Mode::Auto {
                self.errs.push(
                    Diagnostic::error(
                        "E0604",
                        program.span,
                        "only the entry file may declare `mode`",
                    )
                    .with_file(display),
                );
                return Err(());
            }
            if !program.top.is_empty() {
                self.errs.push(
                    Diagnostic::error(
                        "E0604",
                        crate::ast::stmt_span(&program.top[0]),
                        "top-level statements are entry-only (imported files hold items)",
                    )
                    .with_file(display),
                );
                return Err(());
            }
        } else if !apply_entry_script(&mut program, display, &mut self.errs) {
            return Err(());
        }
        // Stamp ownership before recursing (sema reads these, not the parser).
        for f in program.funs.iter_mut() {
            f.module = module.clone();
        }
        for d in program.structs.iter_mut() {
            d.module = module.clone();
        }
        for d in program.enums.iter_mut() {
            d.module = module.clone();
        }
        for i in program.imports.iter_mut() {
            i.from = module.clone();
        }
        // Recurse into imports (source order, deterministic).
        let imps: Vec<(Vec<String>, String, crate::source::Span)> = program
            .imports
            .iter()
            .map(|i| {
                (
                    i.path.clone(),
                    i.items
                        .first()
                        .map(|it| it.name.clone())
                        .unwrap_or_default(),
                    i.items.first().map(|it| it.span).unwrap_or(i.span),
                )
            })
            .collect();
        self.stack.push((norm.clone(), display.to_string()));
        self.units.push(Unit {
            display: display.to_string(),
            module: module.clone(),
            source,
            program,
        });
        self.index.insert(norm, self.units.len() - 1);
        for (ipath, _iname, ispan) in imps {
            let dotted = ipath.join(".");
            let mut rel = PathBuf::new();
            for seg in &ipath {
                rel.push(seg);
            }
            rel.set_extension("valvet");
            // `std::` imports also consult the stdlib roots in order;
            // anything else resolves from the entry directory only.
            // Under a stdlib root the leading `std` segment is dropped
            // (`import std.math::sqrt` lives at `<root>/math.valvet`
            // declaring `module std.math;`). A miss everywhere falls back
            // to the entry-relative path so the diagnostic still names
            // the file the user would create.
            let target = if ipath.first().map(|s| s == "std").unwrap_or(false) {
                let mut hit = self.root.join(&rel);
                if !hit.is_file() && ipath.len() > 1 {
                    let mut srel = PathBuf::new();
                    for seg in &ipath[1..] {
                        srel.push(seg);
                    }
                    srel.set_extension("valvet");
                    for r in &self.stdlib_roots {
                        let cand = r.join(&srel);
                        if cand.is_file() {
                            hit = cand;
                            break;
                        }
                    }
                }
                hit
            } else {
                self.root.join(&rel)
            };
            let target_display = target.display().to_string();
            self.load_unit(&target, &target_display, dotted, Some((display, ispan)))?;
        }
        self.stack.pop();
        Ok(())
    }
}

/// Lexically-normalized absolute path (no fs access; robust to `./`/`../`
/// spellings of the same file for cycle/diamond detection).
fn normalize(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

/// Validate one entry program's script mode and synthesize `fun main`
/// from its top-level statements (dummy ids — the caller runs `renumber`
/// before sema). Returns false with an `E0604` diagnostic when statements
/// meet a user `main`.
pub fn apply_entry_script(
    program: &mut Program,
    display: &str,
    errs: &mut Vec<Diagnostic>,
) -> bool {
    if program.top.is_empty() {
        return true;
    }
    if let Some(m) = program.funs.iter().find(|f| f.name == "main") {
        errs.push(
            Diagnostic::error(
                "E0604",
                m.name_span,
                "entry has both top-level statements and `fun main` (ambiguous entry)",
            )
            .with_help("delete one: statements run as the program, or keep `fun main` alone")
            .with_file(display),
        );
        return false;
    }
    let ret = script_ret(&program.top);
    let span = program.span;
    // Result carriers need a value on fallthrough: scripts end with an
    // implicit `return Ok(0)` (skipped when the last statement already
    // returns). Int32/Unit mains fall through to `return 0` in codegen.
    if matches!(
        ret,
        crate::ast::Ty::Generic { ref base, .. } if base == "Result"
    ) && !matches!(program.top.last(), Some(crate::ast::Stmt::Return { .. }))
    {
        program.top.push(crate::ast::Stmt::Return {
            id: 0,
            value: Some(crate::ast::Expr {
                id: 0,
                kind: crate::ast::ExprKind::Call {
                    name: "Ok".to_string(),
                    args: vec![crate::ast::Expr {
                        id: 0,
                        kind: crate::ast::ExprKind::Int(0),
                        span,
                    }],
                },
                span,
            }),
            span,
        });
    }
    program.funs.push(crate::ast::FunDecl {
        id: 0,
        name: "main".to_string(),
        name_span: span,
        params: vec![],
        ret,
        ret_span: span,
        ret_unit: None,
        body: crate::ast::Block {
            id: 0,
            stmts: std::mem::take(&mut program.top),
            tail: None,
            span,
        },
        span,
        module: String::new(),
        is_pub: false,
    });
    true
}

/// Synthesized `main` return for top-level statements: `Result<Int32,
/// String>` when any `?` occurs (propagation needs a matching carrier),
/// else `Int32` for a valued `return`, else `Unit` (bare `return;` and
/// plain statements; fallthrough exits 0 like a Unit main).
fn script_ret(top: &[crate::ast::Stmt]) -> crate::ast::Ty {
    let mut saw_try = false;
    let mut saw_ret_val = false;
    for s in top {
        scan_stmt(s, &mut saw_try, &mut saw_ret_val);
    }
    if saw_try {
        crate::ast::Ty::Generic {
            base: "Result".to_string(),
            args: vec![crate::ast::Ty::Int32, crate::ast::Ty::Str],
        }
    } else if saw_ret_val {
        crate::ast::Ty::Int32
    } else {
        crate::ast::Ty::Unit
    }
}

fn scan_block(b: &crate::ast::Block, saw_try: &mut bool, saw_ret_val: &mut bool) {
    for s in &b.stmts {
        scan_stmt(s, saw_try, saw_ret_val);
    }
    if let Some(t) = &b.tail {
        scan_expr(t, saw_try, saw_ret_val);
    }
}

fn scan_stmt(s: &crate::ast::Stmt, saw_try: &mut bool, saw_ret_val: &mut bool) {
    use crate::ast::Stmt::*;
    match s {
        Val { init, .. } | Var { init, .. } => scan_expr(init, saw_try, saw_ret_val),
        Expr { expr, .. } => scan_expr(expr, saw_try, saw_ret_val),
        If {
            cond,
            then_b,
            else_b,
            ..
        } => {
            scan_expr(cond, saw_try, saw_ret_val);
            scan_block(then_b, saw_try, saw_ret_val);
            if let Some(b) = else_b {
                scan_block(b, saw_try, saw_ret_val);
            }
        }
        Match {
            scrutinee, arms, ..
        } => {
            scan_expr(scrutinee, saw_try, saw_ret_val);
            for a in arms {
                if let Some(g) = &a.guard {
                    scan_expr(g, saw_try, saw_ret_val);
                }
                scan_block(&a.body, saw_try, saw_ret_val);
            }
        }
        While { cond, body, .. } => {
            scan_expr(cond, saw_try, saw_ret_val);
            scan_block(body, saw_try, saw_ret_val);
        }
        For { iter, body, .. } => {
            match iter {
                crate::ast::ForIter::Range { lo, hi, .. } => {
                    scan_expr(lo, saw_try, saw_ret_val);
                    scan_expr(hi, saw_try, saw_ret_val);
                }
                crate::ast::ForIter::Each { vec, .. } => scan_expr(vec, saw_try, saw_ret_val),
            }
            scan_block(body, saw_try, saw_ret_val);
        }
        Loop { body, .. } => scan_block(body, saw_try, saw_ret_val),
        Break { value, .. } => {
            if let Some(v) = value {
                scan_expr(v, saw_try, saw_ret_val);
            }
        }
        Continue { .. } => {}
        Return { value, .. } => {
            if value.is_some() {
                *saw_ret_val = true;
            }
            if let Some(v) = value {
                scan_expr(v, saw_try, saw_ret_val);
            }
        }
    }
}

fn scan_expr(e: &crate::ast::Expr, saw_try: &mut bool, saw_ret_val: &mut bool) {
    use crate::ast::ExprKind::*;
    match &e.kind {
        Int(_) | Float(_) | Bool(_) | Str(_) | Var(_) => {}
        Call { args, .. } => {
            for a in args {
                scan_expr(a, saw_try, saw_ret_val);
            }
        }
        Binary { lhs, rhs, .. } => {
            scan_expr(lhs, saw_try, saw_ret_val);
            scan_expr(rhs, saw_try, saw_ret_val);
        }
        Unary { expr, .. }
        | Cast { expr, .. }
        | AsUnit { expr, .. }
        | AddrOf { expr, .. }
        | Field { base: expr, .. }
        | OptField { base: expr, .. } => scan_expr(expr, saw_try, saw_ret_val),
        Index { base, idx, .. } => {
            scan_expr(base, saw_try, saw_ret_val);
            scan_expr(idx, saw_try, saw_ret_val);
        }
        Assign { target, value, .. } => {
            scan_place(target, saw_try, saw_ret_val);
            scan_expr(value, saw_try, saw_ret_val);
        }
        If {
            cond,
            then_b,
            else_b,
            ..
        } => {
            scan_expr(cond, saw_try, saw_ret_val);
            scan_block(then_b, saw_try, saw_ret_val);
            if let Some(b) = else_b {
                scan_block(b, saw_try, saw_ret_val);
            }
        }
        Match {
            scrutinee, arms, ..
        } => {
            scan_expr(scrutinee, saw_try, saw_ret_val);
            for a in arms {
                if let Some(g) = &a.guard {
                    scan_expr(g, saw_try, saw_ret_val);
                }
                scan_block(&a.body, saw_try, saw_ret_val);
            }
        }
        Loop { body, .. } => scan_block(body, saw_try, saw_ret_val),
        Block(b) => scan_block(b, saw_try, saw_ret_val),
        Try { expr } => {
            *saw_try = true;
            scan_expr(expr, saw_try, saw_ret_val);
        }
        StructLit { fields, .. } => {
            for (_, _, f) in fields {
                scan_expr(f, saw_try, saw_ret_val);
            }
        }
        VecMacro { elems, .. } => {
            for el in elems {
                scan_expr(el, saw_try, saw_ret_val);
            }
        }
        Method { receiver, args, .. } => {
            scan_expr(receiver, saw_try, saw_ret_val);
            for a in args {
                scan_expr(a, saw_try, saw_ret_val);
            }
        }
        Variant { args, .. } => {
            for a in args {
                scan_expr(a, saw_try, saw_ret_val);
            }
        }
    }
}

fn scan_place(p: &crate::ast::Place, saw_try: &mut bool, saw_ret_val: &mut bool) {
    match p {
        crate::ast::Place::Var(_, _) => {}
        crate::ast::Place::Field { base, .. } | crate::ast::Place::Deref { inner: base, .. } => {
            scan_place(base, saw_try, saw_ret_val)
        }
        crate::ast::Place::Index { base, idx, .. } => {
            scan_place(base, saw_try, saw_ret_val);
            scan_expr(idx, saw_try, saw_ret_val);
        }
    }
}

/// Assign globally-unique `NodeId`s across a joint program. Each source unit
/// parses with ids from 1, so joining without renumbering corrupts every
/// id-keyed table (sema types/sigs, ownership maps). Dense, deterministic
/// (program order). Spans are untouched (file-relative + `.file` attribution).
pub fn renumber(prog: &mut Program) {
    let mut next: crate::ast::NodeId = 1;
    for f in &mut prog.funs {
        f.id = bump(&mut next);
        renumber_block(&mut f.body, &mut next);
    }
    for d in &mut prog.structs {
        d.id = bump(&mut next);
    }
    for d in &mut prog.enums {
        d.id = bump(&mut next);
    }
}

fn bump(next: &mut crate::ast::NodeId) -> crate::ast::NodeId {
    let id = *next;
    *next += 1;
    id
}

fn renumber_block(b: &mut crate::ast::Block, next: &mut crate::ast::NodeId) {
    b.id = bump(next);
    for s in &mut b.stmts {
        renumber_stmt(s, next);
    }
    if let Some(t) = &mut b.tail {
        renumber_expr(t, next);
    }
}

fn renumber_stmt(s: &mut crate::ast::Stmt, next: &mut crate::ast::NodeId) {
    use crate::ast::Stmt::*;
    match s {
        Val { id, init, .. } | Var { id, init, .. } => {
            *id = bump(next);
            renumber_expr(init, next);
        }
        Expr { id, expr, .. } => {
            *id = bump(next);
            renumber_expr(expr, next);
        }
        If {
            id,
            cond,
            then_b,
            else_b,
            ..
        } => {
            *id = bump(next);
            renumber_expr(cond, next);
            renumber_block(then_b, next);
            if let Some(b) = else_b {
                renumber_block(b, next);
            }
        }
        Match {
            id,
            scrutinee,
            arms,
            ..
        } => {
            *id = bump(next);
            renumber_expr(scrutinee, next);
            for a in arms {
                renumber_arm(a, next);
            }
        }
        While { id, cond, body, .. } => {
            *id = bump(next);
            renumber_expr(cond, next);
            renumber_block(body, next);
        }
        For { id, iter, body, .. } => {
            *id = bump(next);
            match iter {
                crate::ast::ForIter::Range { lo, hi, .. } => {
                    renumber_expr(lo, next);
                    renumber_expr(hi, next);
                }
                crate::ast::ForIter::Each { vec } => renumber_expr(vec, next),
            }
            renumber_block(body, next);
        }
        Loop { id, body, .. } => {
            *id = bump(next);
            renumber_block(body, next);
        }
        Break { id, value, .. } | Return { id, value, .. } => {
            *id = bump(next);
            if let Some(v) = value {
                renumber_expr(v, next);
            }
        }
        Continue { id, .. } => {
            *id = bump(next);
        }
    }
}

fn renumber_place(p: &mut crate::ast::Place, next: &mut crate::ast::NodeId) {
    use crate::ast::Place::*;
    match p {
        Var(_, _) => {}
        Field { base, .. } | Deref { inner: base, .. } => renumber_place(base, next),
        Index { base, idx, .. } => {
            renumber_place(base, next);
            renumber_expr(idx, next);
        }
    }
}

fn renumber_arm(a: &mut crate::ast::MatchArm, next: &mut crate::ast::NodeId) {
    // Patterns carry no ids; guard + body do.
    if let Some(g) = &mut a.guard {
        renumber_expr(g, next);
    }
    renumber_block(&mut a.body, next);
}

fn renumber_expr(e: &mut crate::ast::Expr, next: &mut crate::ast::NodeId) {
    use crate::ast::ExprKind::*;
    e.id = bump(next);
    match &mut e.kind {
        Int(_) | Float(_) | Bool(_) | Str(_) | Var(_) => {}
        Call { args, .. } | VecMacro { elems: args } | Variant { args, .. } => {
            for a in args {
                renumber_expr(a, next);
            }
        }
        Method { receiver, args, .. } => {
            renumber_expr(receiver, next);
            for a in args {
                renumber_expr(a, next);
            }
        }
        Binary { lhs, rhs, .. } => {
            renumber_expr(lhs, next);
            renumber_expr(rhs, next);
        }
        Unary { expr, .. }
        | Try { expr }
        | Cast { expr, .. }
        | AsUnit { expr, .. }
        | AddrOf { expr, .. }
        | Field { base: expr, .. }
        | OptField { base: expr, .. } => renumber_expr(expr, next),
        Index { base, idx } => {
            renumber_expr(base, next);
            renumber_expr(idx, next);
        }
        Assign { target, value, .. } => {
            renumber_place(target, next);
            renumber_expr(value, next);
        }
        StructLit { fields, .. } => {
            for (_, _, fe) in fields {
                renumber_expr(fe, next);
            }
        }
        If {
            cond,
            then_b,
            else_b,
        } => {
            renumber_expr(cond, next);
            renumber_block(then_b, next);
            if let Some(b) = else_b {
                renumber_block(b, next);
            }
        }
        Match { scrutinee, arms } => {
            renumber_expr(scrutinee, next);
            for a in arms {
                renumber_arm(a, next);
            }
        }
        Loop { body } => renumber_block(body, next),
        Block(b) => renumber_block(b, next),
    }
}

/// Modules referenced anywhere in the crate (entry `""` + dependencies).
#[allow(dead_code)]
pub fn module_set(krate: &Crate) -> HashSet<String> {
    krate.units.iter().map(|u| u.module.clone()).collect()
}
