use std::collections::HashMap;

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::units::Dim;

/// Symbol info for a local binding.
#[derive(Debug, Clone)]
struct Local {
    ty: Ty,
    mutable: bool,
    dim: Dim,
}

/// How a statement exits: normally, or by diverging control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Return,
    Break,
    Continue,
}

impl Flow {
    fn diverged(self) -> bool {
        !matches!(self, Flow::Normal)
    }
}

/// Innermost enclosing loop while checking a body.
struct LoopCtx {
    /// `true` for `loop` used as a value (may carry `break v`).
    is_expr: bool,
    /// Value types carried by `break v` (must all agree).
    breaks: Vec<Ty>,
    /// A plain (valueless) `break` was seen in this loop.
    plain_break: bool,
}

/// Typechecked program: AST + type of every Expr/Stmt/Block/Fun node.
pub struct Checked {
    pub program: Program,
    /// NodeId -> resolved type (expressions, blocks, statements-as-unit).
    pub types: HashMap<NodeId, Ty>,
    /// FunDecl id -> (param types, ret)
    pub sigs: HashMap<NodeId, (Vec<Ty>, Ty)>,
    /// Names of all functions (for call resolution diagnostics).
    pub fun_names: HashMap<String, NodeId>,
    /// Resolved struct definitions by name.
    pub structs: HashMap<String, StructDef>,
    /// Resolved enum definitions by name.
    pub enums: HashMap<String, EnumDef>,
    /// Builtin generic instantiations by mangled key: (base, arg types).
    /// Only Option/Result resolve in Phase D (`Vec` waits for Phase F).
    pub generic_insts: HashMap<String, (String, Vec<Ty>)>,
    /// NodeId -> resolved dimension (units of measure; absent =
    /// dimensionless). Populated alongside `types`.
    pub dims: HashMap<NodeId, Dim>,
    /// FunDecl id -> parameter dimensions in order (for call conversion).
    pub param_dims: HashMap<NodeId, Vec<Dim>>,
    /// Import aliases: (importing module, local name) -> (target module,
    /// target name). Empty in single-source mode.
    pub aliases: HashMap<(String, String), (String, String)>,
    /// Non-fatal warnings (rendered on success; never fail the build).
    pub warnings: Vec<Diagnostic>,
    /// Sources by load order (entry first) for warning rendering.
    /// Single-source mode holds the one source.
    pub files: Vec<crate::source::Source>,
    /// Composite instantiations printed via `println` (keys into
    /// `generic_insts`); codegen emits format helpers exactly for these.
    pub print_insts: std::collections::HashSet<String>,
    /// Stage-1 functionals: (kind, arg types) kernels for `map`/`filter`/
    /// `fold`/`split_apply` over named functions; codegen emits one C
    /// kernel per entry (deduped).
    pub fun_kernels: Vec<(String, Vec<Ty>)>,
}

/// Structural Copy analysis: an ADT is Copy iff all its fields/payloads are.
/// Whether a type contains poison from an already-reported error.
/// Diagnostic sites check this first and stay silent instead of
/// emitting follow-on (cascade) errors; the poison itself renders as
/// `<error>` only if a guard is ever missed (backstop, never committed).
pub fn ty_has_error(ty: &Ty) -> bool {
    match ty {
        Ty::Error => true,
        Ty::Generic { args, .. } => args.iter().any(ty_has_error),
        Ty::Reference { inner, .. } => ty_has_error(inner),
        _ => false,
    }
}
/// (No recursive types exist in 0.1.0, so this always terminates.)
pub fn is_copy_ty(
    ty: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    match ty {
        Ty::Str | Ty::Named(_) => false,
        // `Vec` owns a heap buffer: copying the header would alias it.
        Ty::Generic { base, .. } if base == "Vec" => false,
        // `Mat` owns its data buffer (see layout in codegen).
        Ty::Generic { base, .. } if base == "Mat" => false,
        // `Factor` owns level/code buffers (see layout in codegen).
        Ty::Generic { base, .. } if base == "Factor" => false,
        // Shared references are Copy; exclusive ones move.
        Ty::Reference { mutable, .. } => !mutable,
        Ty::Generic { args, .. } => args.iter().all(|a| is_copy_ty(a, structs, enums)),
        Ty::Struct(n) => structs
            .get(n)
            .map(|d| d.fields.iter().all(|(_, t)| is_copy_ty(t, structs, enums)))
            .unwrap_or(true),
        Ty::Enum(n) => enums
            .get(n)
            .map(|d| {
                d.variants
                    .iter()
                    .all(|(_, ts)| ts.iter().all(|t| is_copy_ty(t, structs, enums)))
            })
            .unwrap_or(true),
        _ => true,
    }
}

pub fn check(program: &Program) -> (Option<Checked>, Vec<Diagnostic>) {
    check_with_files(program, &HashMap::new())
}

/// Joint-program check over a loaded crate: `mod_files` maps owning modules
/// (`""` for the entry) to display paths for per-file diagnostics.
pub fn check_with_files(
    program: &Program,
    mod_files: &HashMap<String, String>,
) -> (Option<Checked>, Vec<Diagnostic>) {
    // Work on an owned clone so nominal types resolve in place; everything
    // downstream (ownership, codegen) then sees only concrete types.
    let mut prog = program.clone();
    let mut cx = Cx {
        errs: vec![],
        types: HashMap::new(),
        sigs: HashMap::new(),
        fun_names: HashMap::new(),
        locals: vec![],
        loop_stack: vec![],
        fn_ret: Ty::Unit,
        fn_ret_dim: Dim::dimensionless(),
        fn_params: std::collections::HashSet::new(),
        expected: None,
        structs: HashMap::new(),
        enums: HashMap::new(),
        generic_insts: HashMap::new(),
        fun_kernels: vec![],
        cur_module: String::new(),
        cur_file: String::new(),
        mod_files: mod_files.clone(),
        aliases: HashMap::new(),
        warns: vec![],
        print_insts: std::collections::HashSet::new(),
        dims: HashMap::new(),
        param_dims: HashMap::new(),
    };
    // Builtin `ShapeError` for Vec kernels (user redefinition is E0401).
    cx.structs.insert(
        "ShapeError".to_string(),
        StructDef {
            fields: vec![
                ("expected".to_string(), Ty::Int64),
                ("found".to_string(), Ty::Int64),
            ],
            is_tuple: false,
        },
    );
    // Register signatures first (forward calls allowed). Keys are
    // module-qualified (`m::f`; entry keeps plain `f`), so same-named
    // private functions coexist across modules.
    for f in &program.funs {
        cx.set_mod(&f.module.clone());
        if f.name == "main" && !f.module.is_empty() {
            cx.err(
                "E0401",
                f.name_span,
                "only the entry file may define `fun main()`",
                Some("rename this function or move it to the entry file".into()),
            );
            continue;
        }
        let q = crate::resolver::qname(&f.module, &f.name);
        if cx.fun_names.contains_key(&q) {
            cx.err(
                "E0401",
                f.name_span,
                format!("duplicate function `{}`", f.name),
                Some("rename one of the functions".into()),
            );
            continue;
        }
        cx.fun_names.insert(q, f.id);
        cx.sigs.insert(
            f.id,
            (
                f.params.iter().map(|p| p.ty.clone()).collect(),
                f.ret.clone(),
            ),
        );
    }
    // Rejected non-entry `main`s leave the joint program (downstream would
    // emit a second C `main` and check unchecked bodies).
    prog.funs
        .retain(|f| !(f.name == "main" && !f.module.is_empty()));
    cx.set_mod("");
    if program
        .funs
        .iter()
        .filter(|f| f.module.is_empty())
        .all(|f| f.name != "main")
    {
        // Entry point is required to *link* an executable, but analysis continues so
        // that all other diagnostics are still reported (harness matches expected codes).
        cx.err(
            "E0603",
            program.span,
            "missing program entry point `fun main()`",
            Some("add `fun main() { ... }`".into()),
        );
    }
    // Phase 1b: register ADT names (empty shells for recursion checks).
    cx.register_adts(&prog);
    // Phase 1c: bind imports to qualified targets (visibility checked here).
    cx.bind_imports(program);
    // Phase 2: resolve struct fields + enum payloads (rejects cycles).
    cx.resolve_adts(&mut prog);
    // Phase 3: resolve function signatures.
    cx.resolve_sigs(&mut prog);
    // Phase 4: check function bodies (non-entry `main`s were rejected at
    // registration; skipping keeps them out of the linked TU).
    for i in 0..prog.funs.len() {
        let f = prog.funs[i].clone();
        if f.name == "main" && !f.module.is_empty() {
            continue;
        }
        cx.check_fun(&f);
    }
    if !cx.errs.is_empty() {
        return (None, cx.errs);
    }
    let sigs2 = cx.sigs.clone();
    let aliases2 = cx.aliases.clone();
    let names2 = cx.fun_names.clone();
    let structs2 = cx.structs.clone();
    let enums2 = cx.enums.clone();
    let insts2 = cx.generic_insts.clone();
    let pdims2 = cx.param_dims.clone();
    (
        Some(Checked {
            program: prog,
            types: cx.types,
            sigs: sigs2,
            aliases: aliases2,
            fun_names: names2,
            structs: structs2,
            enums: enums2,
            generic_insts: insts2,
            warnings: cx.warns,
            // Pipeline stamps owning sources after (sema never sees text).
            files: vec![],
            print_insts: cx.print_insts,
            fun_kernels: cx.fun_kernels,
            dims: cx.dims,
            param_dims: pdims2,
        }),
        vec![],
    )
}

struct Cx {
    errs: Vec<Diagnostic>,
    types: HashMap<NodeId, Ty>,
    sigs: HashMap<NodeId, (Vec<Ty>, Ty)>,
    fun_names: HashMap<String, NodeId>,
    locals: Vec<HashMap<String, Local>>,
    loop_stack: Vec<LoopCtx>,
    /// Enclosing function return type (for `return` inside value-`loop` bodies).
    fn_ret: Ty,
    /// Enclosing function return dimension (`@unit` on the return type).
    fn_ret_dim: Dim,
    /// Enclosing function parameter names (for return-dangling checks).
    fn_params: std::collections::HashSet<String>,
    /// Expected type for constructor inference (`None`, qualified Result
    /// ctors): set by annotated positions (let/call-arg/return) around the
    /// value check, read by the `Variant` arm. Cleared immediately after.
    expected: Option<Ty>,
    structs: HashMap<String, StructDef>,
    enums: HashMap<String, EnumDef>,
    generic_insts: HashMap<String, (String, Vec<Ty>)>,
    /// Module whose items are being registered/checked ("" for the entry).
    cur_module: String,
    /// Display path of `cur_module` ("" when unknown — diagnostics unstamped).
    cur_file: String,
    /// Owning module -> display path (empty in single-source mode).
    mod_files: HashMap<String, String>,
    /// (importing module, local name) -> (target module, target name).
    /// Bound once (visibility enforced at bind time, so every alias is
    /// usable). Exported to codegen via `Checked::aliases`.
    aliases: HashMap<(String, String), (String, String)>,
    /// Non-fatal warnings (e.g. deprecated `print`); exported via Checked.
    warns: Vec<Diagnostic>,
    /// Composite instantiations actually printed (`println(v)`); codegen
    /// emits `vfmt_`/`vprint_` helpers exactly for these (gated, like kernels).
    print_insts: std::collections::HashSet<String>,
    /// Stage-1 functional kernels (see `Checked::fun_kernels`).
    fun_kernels: Vec<(String, Vec<Ty>)>,
    /// NodeId -> resolved dimension (units of measure; absent =
    /// dimensionless). Exported via `Checked::dims` for scale conversions.
    dims: HashMap<NodeId, Dim>,
    /// FunDecl id -> parameter dimensions (exported via `Checked`).
    param_dims: HashMap<NodeId, Vec<Dim>>,
}

/// Operands for `check_binary` (bundled: clippy arity limit).
struct BinCheck<'a> {
    id: NodeId,
    span: Span,
    op: BinOp,
    lt: &'a Ty,
    rt: &'a Ty,
    ld: &'a Dim,
    rd: &'a Dim,
    lhs: Option<&'a Expr>,
    rhs: Option<&'a Expr>,
}

/// Whether a statement tree contains a `return` at any depth. Valvet has no
/// closures, so every `return` is function-level; value-position blocks
/// (`check_block_expr`) reject them with E0800 instead of checking against
/// a dummy `()` return type (which produced a bogus "`return` value is T,
/// but function returns ()").
/// Builtin nominal type names share C type names with user ADTs; user
/// redefinitions are rejected (E0401) in `register_adts` instead of
/// shadowing (shadowing produced conflicting C declarations).
fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "Int32"
            | "Int64"
            | "Float64"
            | "Bool"
            | "String"
            | "Unit"
            | "Vec"
            | "Option"
            | "Result"
            | "Missing"
            | "Rng"
            | "Mat"
            | "Factor"
    )
}

fn stmt_tree_has_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return { .. } => true,
        Stmt::Val { init, .. } | Stmt::Var { init, .. } => expr_tree_has_return(init),
        Stmt::Expr { expr, .. } => expr_tree_has_return(expr),
        Stmt::If {
            cond,
            then_b,
            else_b,
            ..
        } => {
            expr_tree_has_return(cond)
                || block_tree_has_return(then_b)
                || else_b.as_ref().is_some_and(|b| block_tree_has_return(b))
        }
        Stmt::Match {
            scrutinee, arms, ..
        } => {
            expr_tree_has_return(scrutinee)
                || arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(expr_tree_has_return)
                        || block_tree_has_return(&a.body)
                })
        }
        Stmt::While { cond, body, .. } => expr_tree_has_return(cond) || block_tree_has_return(body),
        Stmt::For { iter, body, .. } => {
            let it = match iter {
                ForIter::Range { lo, hi, .. } => {
                    expr_tree_has_return(lo) || expr_tree_has_return(hi)
                }
                ForIter::Each { vec } => expr_tree_has_return(vec),
            };
            it || block_tree_has_return(body)
        }
        Stmt::Loop { body, .. } => block_tree_has_return(body),
        Stmt::LetElse {
            scrutinee, else_b, ..
        } => expr_tree_has_return(scrutinee) || block_tree_has_return(else_b),
        Stmt::Break { value, .. } => value.as_ref().is_some_and(expr_tree_has_return),
        Stmt::Continue { .. } => false,
    }
}

fn block_tree_has_return(b: &Block) -> bool {
    b.stmts.iter().any(stmt_tree_has_return)
        || b.tail.as_ref().is_some_and(|t| expr_tree_has_return(t))
}

fn expr_tree_has_return(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Var(_) => false,
        ExprKind::Call { args, .. } | ExprKind::Variant { args, .. } => {
            args.iter().any(expr_tree_has_return)
        }
        ExprKind::Binary { lhs, rhs, .. } => expr_tree_has_return(lhs) || expr_tree_has_return(rhs),
        ExprKind::Unary { expr, .. }
        | ExprKind::Cast { expr, .. }
        | ExprKind::AsUnit { expr, .. }
        | ExprKind::AddrOf { expr, .. }
        | ExprKind::Field { base: expr, .. }
        | ExprKind::OptField { base: expr, .. }
        | ExprKind::Try { expr } => expr_tree_has_return(expr),
        ExprKind::Index { base, idx, .. } => {
            expr_tree_has_return(base) || expr_tree_has_return(idx)
        }
        ExprKind::Assign { value, .. } => expr_tree_has_return(value),
        ExprKind::If {
            cond,
            then_b,
            else_b,
            ..
        } => {
            expr_tree_has_return(cond)
                || block_tree_has_return(then_b)
                || else_b.as_ref().is_some_and(|b| block_tree_has_return(b))
        }
        ExprKind::Match {
            scrutinee, arms, ..
        } => {
            expr_tree_has_return(scrutinee)
                || arms.iter().any(|a| {
                    a.guard.as_ref().is_some_and(expr_tree_has_return)
                        || block_tree_has_return(&a.body)
                })
        }
        ExprKind::Loop { body, .. } => block_tree_has_return(body),
        ExprKind::Block(b) => block_tree_has_return(b),
        ExprKind::StructLit { fields, .. } => {
            fields.iter().any(|(_, _, f)| expr_tree_has_return(f))
        }
        ExprKind::VecMacro { elems, .. } => elems.iter().any(expr_tree_has_return),
        ExprKind::Method { receiver, args, .. } => {
            expr_tree_has_return(receiver) || args.iter().any(expr_tree_has_return)
        }
    }
}

impl Cx {
    fn err(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        help: Option<String>,
    ) {
        let mut d = Diagnostic::error(code, span, msg);
        if let Some(h) = help {
            d = d.with_help(h);
        }
        // Multi-file mode: attribute to the module under registration/check
        // so errors render against the right file (stays None single-source).
        if !self.cur_file.is_empty() {
            d = d.with_file(self.cur_file.clone());
        }
        // dedupe
        if let Some(l) = self.errs.last() {
            if l.code == code && l.message == d.message && l.span == d.span {
                return;
            }
        }
        self.errs.push(d);
    }

    /// Push a non-fatal warning (file-stamped like errors, but compilation
    /// continues and succeeds).
    fn warn(&mut self, code: &'static str, span: Span, msg: impl Into<String>) {
        let mut d = Diagnostic::warning(code, span, msg);
        if !self.cur_file.is_empty() {
            d = d.with_file(self.cur_file.clone());
        }
        self.warns.push(d);
    }

    /// Whether a place contains an index layer (nested index stores are
    /// rejected: only the outermost index carries a bounds check).
    fn place_has_index(p: &Place) -> bool {
        match p {
            Place::Index { .. } => true,
            Place::Field { base, .. } => Self::place_has_index(base),
            Place::Deref { inner, .. } => Self::place_has_index(inner),
            Place::Var(_, _) => false,
        }
    }

    /// Resolve an assignment target to (type, mutable?, dimension). `var`
    /// roots need mutable bindings; `*r` needs `&mut`; field paths recurse.
    /// Only `Var` roots and dereferences carry dimensions (fields and
    /// indices are dimensionless: neither can be `@`-annotated).
    fn check_place(&mut self, target: &Place) -> Option<(Ty, bool, Dim)> {
        match target {
            Place::Var(name, span) => match self.lookup(name).cloned() {
                None => {
                    self.err("E0401", *span, format!("unknown variable `{}`", name), None);
                    None
                }
                Some(l) => Some((l.ty, l.mutable, l.dim)),
            },
            Place::Field { base, field, span } => {
                let (bt, bmut, _) = self.check_place(base)?;
                // Auto-deref: field access through a reference resolves into
                // the referent struct (mirrors the expression-position rule).
                // Mutability then comes from the borrow (`&mut`), not the
                // holder binding (`val m = &mut p` writes through `m.y`).
                let (bt, bmut) = match bt {
                    Ty::Reference { mutable, inner, .. } => (*inner.clone(), mutable),
                    t => (t, bmut),
                };
                match bt {
                    Ty::Struct(sn) => match self.structs.get(&sn).cloned() {
                        Some(def) => match def.fields.iter().find(|(f, _)| f == field) {
                            Some((_, fty)) => Some((fty.clone(), bmut, Dim::dimensionless())),
                            None => {
                                self.err(
                                    "E0401",
                                    *span,
                                    format!("unknown field `{}.{}`", sn, field),
                                    None,
                                );
                                None
                            }
                        },
                        None => None,
                    },
                    _ => {
                        if !ty_has_error(&bt) {
                            self.err(
                                "E0201",
                                *span,
                                format!("type {} has no fields", bt.name()),
                                None,
                            );
                        }
                        None
                    }
                }
            }
            Place::Deref { inner, span } => {
                let (bt, _, bdim) = self.check_place(inner)?;
                match bt {
                    Ty::Reference {
                        mutable: true,
                        inner,
                        ..
                    } => Some((*inner.clone(), true, bdim)),
                    Ty::Reference { mutable: false, .. } => {
                        self.err(
                            "E0201",
                            *span,
                            "cannot assign through a shared (`&`) reference; use `&mut`",
                            None,
                        );
                        None
                    }
                    _ => {
                        if !ty_has_error(&bt) {
                            self.err(
                                "E0201",
                                *span,
                                format!("cannot dereference {}", bt.name()),
                                None,
                            );
                        }
                        None
                    }
                }
            }
            Place::Index { base, idx, span } => {
                if Self::place_has_index(base) {
                    self.err(
                        "E0201",
                        *span,
                        "nested index assignment is not supported (bind the inner element first)",
                        None,
                    );
                    return None;
                }
                let (bt, bmut, _) = self.check_place(base)?;
                if ty_has_error(&bt) {
                    return None;
                }
                match bt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        let it = self.check_expr(idx);
                        if !matches!(it, Ty::Int32 | Ty::Int64) && !ty_has_error(&it) {
                            self.err(
                                "E0201",
                                idx.span,
                                format!("vector index must be Int32/Int64, found {}", it.name()),
                                None,
                            );
                        }
                        Some((args[0].clone(), bmut, Dim::dimensionless()))
                    }
                    _ => {
                        self.err(
                            "E0401",
                            *span,
                            format!(
                                "type {} cannot be indexed (only `Vec` supports `v[i]`)",
                                bt.name()
                            ),
                            None,
                        );
                        None
                    }
                }
            }
        }
    }

    /// Root variable of a borrowable place (`x`, `a.b.c`, `*r`, ...).
    /// Anything else (calls, literals, operators) is a temporary.
    fn borrow_root(e: &Expr) -> Option<String> {
        match &e.kind {
            ExprKind::Var(n) => Some(n.clone()),
            ExprKind::Field { base, .. } => Self::borrow_root(base),
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => Self::borrow_root(expr),
            _ => None,
        }
    }

    fn lookup(&self, name: &str) -> Option<&Local> {
        for scope in self.locals.iter().rev() {
            if let Some(l) = scope.get(name) {
                return Some(l);
            }
        }
        None
    }

    /// Enter a module's registration/check context (file attribution follows).
    fn set_mod(&mut self, module: &str) {
        self.cur_module = module.to_string();
        self.cur_file = self.mod_files.get(module).cloned().unwrap_or_default();
    }

    /// Resolve a function name from the current module: own items first, then
    /// imports (bound only when `pub`, so every hit is callable). Returns the
    /// FunDecl id for the shared signature table.
    fn resolve_fun(&self, name: &str) -> Option<NodeId> {
        let own = crate::resolver::qname(&self.cur_module, name);
        if let Some(id) = self.fun_names.get(&own) {
            return Some(*id);
        }
        if let Some((tm, tn)) = self
            .aliases
            .get(&(self.cur_module.clone(), name.to_string()))
        {
            let tq = crate::resolver::qname(tm, tn);
            return self.fun_names.get(&tq).copied();
        }
        None
    }

    /// Resolve a type name like `resolve_fun` (structs + enums share one
    /// namespace). Returns the qualified name for the global tables.
    fn resolve_type(&self, name: &str) -> Option<String> {
        let own = crate::resolver::qname(&self.cur_module, name);
        if self.structs.contains_key(&own) || self.enums.contains_key(&own) {
            return Some(own);
        }
        if let Some((tm, tn)) = self
            .aliases
            .get(&(self.cur_module.clone(), name.to_string()))
        {
            let tq = crate::resolver::qname(tm, tn);
            if self.structs.contains_key(&tq) || self.enums.contains_key(&tq) {
                return Some(tq);
            }
        }
        None
    }

    /// Bind every import to its target qualified name (Phase H, runs once
    /// after registration, before bodies). Checks: target module loaded,
    /// name exists there, `pub` visibility, no alias clashes. All E0401
    /// except a missing module (E0701 — resolver guarantees files, but an
    /// empty module declares no items to anchor on).
    fn bind_imports(&mut self, prog: &Program) {
        // (module, name) -> (qualified, is_pub) over all declarations.
        let mut decls: HashMap<(String, String), (String, bool)> = HashMap::new();
        for f in &prog.funs {
            // Non-entry `main`s were skipped at registration; never importable.
            if f.name == "main" && !f.module.is_empty() {
                continue;
            }
            decls.insert(
                (f.module.clone(), f.name.clone()),
                (crate::resolver::qname(&f.module, &f.name), f.is_pub),
            );
        }
        for d in &prog.structs {
            decls.insert(
                (d.module.clone(), d.name.clone()),
                (crate::resolver::qname(&d.module, &d.name), d.is_pub),
            );
        }
        for d in &prog.enums {
            decls.insert(
                (d.module.clone(), d.name.clone()),
                (crate::resolver::qname(&d.module, &d.name), d.is_pub),
            );
        }
        let modules: std::collections::HashSet<&str> =
            decls.keys().map(|(m, _)| m.as_str()).collect();
        for imp in &prog.imports {
            self.set_mod(&imp.from);
            let target_mod = imp.path.join(".");
            for item in &imp.items {
                let local = item.local().to_string();
                // Alias clash: own item or an earlier alias in this module.
                let own_hit = decls.contains_key(&(imp.from.clone(), local.clone()));
                let alias_hit = self
                    .aliases
                    .contains_key(&(imp.from.clone(), local.clone()));
                if own_hit || alias_hit {
                    self.err(
                        "E0401",
                        item.span,
                        format!("imported name `{}` clashes with a local definition", local),
                        Some("rename the import with `as`".into()),
                    );
                    continue;
                }
                match decls.get(&(target_mod.clone(), item.name.clone())) {
                    None => {
                        // Distinguish "no such module" from "no such item".
                        if !modules.contains(target_mod.as_str()) && target_mod != imp.from {
                            self.err(
                                "E0701",
                                item.span,
                                format!(
                                    "unresolved module `{}` (no such module loaded)",
                                    target_mod
                                ),
                                None,
                            );
                        } else {
                            self.err(
                                "E0401",
                                item.span,
                                format!("unknown name `{}` in module `{}`", item.name, target_mod),
                                None,
                            );
                        }
                    }
                    Some((_, is_pub)) => {
                        if !is_pub {
                            self.err(
                                "E0401",
                                item.span,
                                format!(
                                    "`{}` is private in module `{}` (declare it `pub`)",
                                    item.name, target_mod
                                ),
                                None,
                            );
                            continue;
                        }
                        self.aliases.insert(
                            (imp.from.clone(), local),
                            (target_mod.clone(), item.name.clone()),
                        );
                    }
                }
            }
        }
        self.set_mod("");
    }

    /// Phase 1b: register struct/enum names so fields can reference them.
    fn register_adts(&mut self, prog: &Program) {
        for d in &prog.structs {
            self.set_mod(&d.module);
            let q = crate::resolver::qname(&d.module, &d.name);
            if is_builtin_type_name(&d.name) {
                // Shadowing a builtin type breaks codegen (shared C type
                // names), so it is rejected rather than shadowed.
                self.err(
                    "E0401",
                    d.name_span,
                    format!(
                        "`{}` is a builtin type name and cannot be redefined",
                        d.name
                    ),
                    Some("rename the type".into()),
                );
                continue;
            }
            if self.structs.contains_key(&q)
                || self.enums.contains_key(&q)
                || self.fun_names.contains_key(&q)
            {
                self.err(
                    "E0401",
                    d.name_span,
                    format!("duplicate definition of `{}`", d.name),
                    None,
                );
                continue;
            }
            let def = match &d.kind {
                StructKind::Named(fs) => StructDef {
                    fields: fs.iter().map(|f| (f.name.clone(), f.ty.clone())).collect(),
                    is_tuple: false,
                },
                StructKind::Tuple(ts) => StructDef {
                    fields: ts
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (format!("_{}", i), t.clone()))
                        .collect(),
                    is_tuple: true,
                },
                StructKind::Unit => StructDef {
                    fields: vec![],
                    is_tuple: false,
                },
            };
            self.structs.insert(q, def);
        }
        for d in &prog.enums {
            self.set_mod(&d.module);
            let q = crate::resolver::qname(&d.module, &d.name);
            if is_builtin_type_name(&d.name) {
                self.err(
                    "E0401",
                    d.name_span,
                    format!(
                        "`{}` is a builtin type name and cannot be redefined",
                        d.name
                    ),
                    Some("rename the type".into()),
                );
                continue;
            }
            if self.enums.contains_key(&q)
                || self.structs.contains_key(&q)
                || self.fun_names.contains_key(&q)
            {
                self.err(
                    "E0401",
                    d.name_span,
                    format!("duplicate definition of `{}`", d.name),
                    None,
                );
                continue;
            }
            let mut seen = std::collections::HashSet::new();
            let mut variants = vec![];
            for v in &d.variants {
                if !seen.insert(v.name.clone()) {
                    self.err(
                        "E0401",
                        v.span,
                        format!("duplicate variant `{}`", v.name),
                        None,
                    );
                    continue;
                }
                variants.push((v.name.clone(), v.payload.clone()));
            }
            self.enums.insert(q, EnumDef { variants });
        }
        self.set_mod("");
    }

    /// Mangled key for a builtin generic instantiation.
    fn inst_key(base: &str, args: &[Ty]) -> String {
        let inner: Vec<String> = args.iter().map(Self::ty_key).collect();
        format!("{}<{}>", base, inner.join(","))
    }

    /// Record a `Vec<T>` instantiation for codegen.
    fn record_vec_inst(&mut self, t: &Ty) {
        // Full recording (outer + nested inners + `Option<T>` pop/get support):
        // `Vec<Option<Int32>>` needs the inner `Option<Int32>` layout too.
        self.record_ty(&Ty::Generic {
            base: "Vec".to_string(),
            args: vec![t.clone()],
        });
    }

    /// Record a result type and any nested instantiations (`Result<Vec<T>, E>`
    /// needs all three layouts). Idempotent. A `Vec<T>` additionally implies
    /// `Option<T>` (pop/get signatures reference it).
    fn record_ty(&mut self, ty: &Ty) {
        if let Ty::Generic { base, args } = ty {
            if base == "Vec"
                || base == "Option"
                || base == "Result"
                || base == "Missing"
                || base == "Rng"
                || base == "Mat"
                || base == "Factor"
            {
                for a in args {
                    self.record_ty(a);
                }
                let key = Self::inst_key(base, args);
                self.generic_insts.insert(key, (base.clone(), args.clone()));
                if base == "Vec" && args.len() == 1 {
                    let opt = Ty::Generic {
                        base: "Option".to_string(),
                        args: args.clone(),
                    };
                    let key = Self::inst_key("Option", args);
                    self.generic_insts
                        .insert(key, ("Option".to_string(), args.clone()));
                    let _ = opt;
                }
            }
        }
    }

    /// Key fragment for one (resolved) type argument.
    fn ty_key(ty: &Ty) -> String {
        match ty {
            Ty::Int32 => "i32".to_string(),
            Ty::Int64 => "i64".to_string(),
            Ty::Float64 => "f64".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Str => "str".to_string(),
            Ty::Unit => "unit".to_string(),
            Ty::Named(n) => format!("unresolved:{}", n),
            Ty::Struct(n) | Ty::Enum(n) => n.clone(),
            // No nested references in 0.1.0; total fallback.
            Ty::Reference { .. } => "ref".to_string(),
            Ty::Generic { base, args } => Self::inst_key(base, args),
            // Unreachable on success paths (errors abort before lowering).
            Ty::Error => "error".to_string(),
        }
    }

    /// Collect top-level borrow lifetimes in a type (explicit names or
    /// `None` placeholders for elided positions).
    /// Resolve a (possibly nominal/generic) type against primitives +
    /// registered ADTs. Records builtin instantiations for codegen.
    fn resolve_ty(&mut self, ty: &Ty, span: Span) -> Ty {
        match ty {
            Ty::Reference {
                lifetime,
                mutable,
                inner,
            } => {
                let r = self.resolve_ty(inner, span);
                if has_ref(&r) {
                    self.err(
                        "E0201",
                        span,
                        "nested references are deferred in 0.1.0 (references only at top level)",
                        None,
                    );
                    return Ty::Error;
                }
                Ty::Reference {
                    lifetime: lifetime.clone(),
                    mutable: *mutable,
                    inner: Box::new(r),
                }
            }
            Ty::Generic { base, args } => {
                let mut resolved = Vec::with_capacity(args.len());
                for a in args {
                    resolved.push(self.resolve_ty(a, span));
                }
                if resolved.iter().any(has_ref) {
                    self.err(
                        "E0201",
                        span,
                        "references inside generic arguments are deferred in 0.1.0",
                        None,
                    );
                    return Ty::Error;
                }
                match (base.as_str(), resolved.len()) {
                    ("Option", 1)
                    | ("Result", 2)
                    | ("Vec", 1)
                    | ("Missing", 1)
                    | ("Rng", 0)
                    | ("Mat", 0)
                    | ("Factor", 0) => {
                        let t = Ty::Generic {
                            base: base.clone(),
                            args: resolved,
                        };
                        self.record_ty(&t);
                        t
                    }
                    _ => {
                        self.err(
                            "E0201",
                            span,
                            format!("unknown generic type `{}`, or wrong arity", base),
                            Some("only `Option<T>`, `Result<T, E>`, `Vec<T>`, `Missing<T>`, `Rng`, `Mat` and `Factor` are supported".into()),
                        );
                        Ty::Error
                    }
                }
            }
            Ty::Named(n) => match n.as_str() {
                // Bare `Mat`/`Rng` annotations (parser only makes Generics
                // with `<...>`; these arity-0 builtins never take args).
                "Mat" | "Rng" | "Factor" => {
                    let t = Ty::Generic {
                        base: n.clone(),
                        args: vec![],
                    };
                    self.record_ty(&t);
                    t
                }
                _ => match self.resolve_type(n) {
                    Some(q) if self.structs.contains_key(&q) => Ty::Struct(q),
                    Some(q) => Ty::Enum(q),
                    None => {
                        self.err(
                            "E0201",
                            span,
                            format!("unknown type `{}`", n),
                            Some("expected Int32, Int64, Float64, Bool, String, or a declared struct/enum".into()),
                        );
                        Ty::Error
                    }
                },
            },
            other => other.clone(),
        }
    }

    /// Phase 2: resolve ADT field/payload types in place; reject type cycles
    /// (no Box/pointer indirection exists in 0.1.0, so any cycle is unsized).
    fn resolve_adts(&mut self, prog: &mut Program) {
        for d in &prog.structs {
            // Field types may reference imported types: resolve in the
            // declaring module's context.
            self.set_mod(&d.module.clone());
            // Field spans for diagnostics (tuple fields reuse the name span).
            let fspans: Vec<Span> = match &d.kind {
                StructKind::Named(fs) => fs.iter().map(|f| f.span).collect(),
                StructKind::Tuple(ts) => vec![d.name_span; ts.len()],
                StructKind::Unit => vec![],
            };
            let q = crate::resolver::qname(&d.module, &d.name);
            let current: Vec<Ty> = match self.structs.get(&q) {
                Some(x) => x.fields.iter().map(|(_, t)| t.clone()).collect(),
                None => continue, // duplicate; already reported
            };
            let mut resolved = Vec::with_capacity(current.len());
            for (i, r) in current.iter().enumerate() {
                let sp = fspans.get(i).copied().unwrap_or(d.name_span);
                resolved.push(self.resolve_ty(r, sp));
            }
            if resolved.iter().any(has_ref) {
                self.err(
                    "E0201",
                    d.name_span,
                    format!(
                        "references in struct `{}` fields are deferred in 0.1.0",
                        d.name
                    ),
                    None,
                );
            }
            let q = crate::resolver::qname(&d.module, &d.name);
            if let Some(def) = self.structs.get_mut(&q) {
                for ((_, fty), r) in def.fields.iter_mut().zip(resolved) {
                    *fty = r;
                }
            }
        }
        for d in &prog.enums {
            self.set_mod(&d.module.clone());
            // Payload spans are not tracked; use the variant span.
            let vspans: Vec<Span> = d.variants.iter().map(|v| v.span).collect();
            let q = crate::resolver::qname(&d.module, &d.name);
            let current: Vec<Vec<Ty>> = match self.enums.get(&q) {
                Some(x) => x.variants.iter().map(|(_, ts)| ts.clone()).collect(),
                None => continue, // duplicate; already reported
            };
            let mut resolved_all = Vec::with_capacity(current.len());
            for (i, pts) in current.iter().enumerate() {
                let sp = vspans.get(i).copied().unwrap_or(d.name_span);
                let mut out = Vec::with_capacity(pts.len());
                for pt in pts {
                    out.push(self.resolve_ty(pt, sp));
                }
                resolved_all.push(out);
            }
            if resolved_all.iter().any(|r| r.iter().any(has_ref)) {
                self.err(
                    "E0201",
                    d.name_span,
                    format!(
                        "references in enum `{}` payloads are deferred in 0.1.0",
                        d.name
                    ),
                    None,
                );
            }
            let q = crate::resolver::qname(&d.module, &d.name);
            if let Some(def) = self.enums.get_mut(&q) {
                for ((_, pts), r) in def.variants.iter_mut().zip(resolved_all) {
                    *pts = r;
                }
            }
        }
        // Cycle check over the nominal dependency graph.
        let mut color: HashMap<String, u8> = HashMap::new();
        let structs = self.structs.clone();
        let enums = self.enums.clone();
        let mut spans: HashMap<String, Span> = HashMap::new();
        for d in &prog.structs {
            spans.insert(crate::resolver::qname(&d.module, &d.name), d.name_span);
        }
        for d in &prog.enums {
            spans.insert(crate::resolver::qname(&d.module, &d.name), d.name_span);
        }
        for name in structs
            .keys()
            .chain(enums.keys().filter(|n| !structs.contains_key(*n)))
        {
            if color.get(name).copied().unwrap_or(0) == 0 {
                self.visit_ty(name, &structs, &enums, &spans, &mut color, &mut vec![]);
            }
        }
    }

    /// DFS for nominal cycles; reports E0201 on the closing edge.
    /// Nominal types a type depends on (recursive through builtin generics:
    /// indirection through `Vec`/`Option`/`Result` does not exist in 0.1.0 —
    /// references are top-level-only — so cycles through them are unsized).
    fn nominal_deps(ty: &Ty, deps: &mut Vec<(String, Span)>) {
        match ty {
            Ty::Struct(n) | Ty::Enum(n) => deps.push((n.clone(), Span::empty(0))),
            Ty::Generic { args, .. } => {
                for a in args {
                    Self::nominal_deps(a, deps);
                }
            }
            _ => {}
        }
    }

    fn visit_ty(
        &mut self,
        name: &str,
        structs: &HashMap<String, StructDef>,
        enums: &HashMap<String, EnumDef>,
        spans: &HashMap<String, Span>,
        color: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
    ) {
        color.insert(name.to_string(), 1);
        stack.push(name.to_string());
        let mut deps: Vec<(String, Span)> = vec![];
        if let Some(d) = structs.get(name) {
            for (_, t) in &d.fields {
                Self::nominal_deps(t, &mut deps);
            }
        }
        if let Some(d) = enums.get(name) {
            for (_, ts) in &d.variants {
                for t in ts {
                    Self::nominal_deps(t, &mut deps);
                }
            }
        }
        for (dep, _) in deps {
            match color.get(&dep).copied().unwrap_or(0) {
                0 => self.visit_ty(&dep, structs, enums, spans, color, stack),
                1 => {
                    let mut cycle = stack.clone();
                    cycle.push(dep.clone());
                    let sp = spans.get(&dep).copied().unwrap_or(Span::empty(0));
                    self.err(
                        "E0201",
                        sp,
                        format!(
                            "recursive type `{}` has infinite size (no indirection exists yet)",
                            cycle.join(" -> ")
                        ),
                        Some("remove the cycle; Box/pointer indirection is deferred".into()),
                    );
                }
                _ => {}
            }
        }
        stack.pop();
        color.insert(name.to_string(), 2);
    }

    /// Phase 3: resolve function parameter/return types in place + refresh sigs.
    fn resolve_sigs(&mut self, prog: &mut Program) {
        for f in &mut prog.funs {
            // set_mod borrows self immutably first (bulk borrowck workaround).
            let m = f.module.clone();
            self.set_mod(&m);
            for p in &mut f.params {
                let r = p.ty.clone();
                p.ty = self.resolve_ty(&r, p.span);
            }
            let r = f.ret.clone();
            f.ret = self.resolve_ty(&r, f.ret_span);
            if let Some(sig) = self.sigs.get_mut(&f.id) {
                *sig = (
                    f.params.iter().map(|p| p.ty.clone()).collect(),
                    f.ret.clone(),
                );
            }
        }
    }

    /// Lifetime elision + well-formedness for one signature (0.1.0 rules):
    /// collect top-level borrow lifetimes of params (explicit names or one
    /// fresh elided region per position — unified here as distinct slots);
    /// an elided output lifetime needs exactly one input lifetime, else the
    /// output must name an input lifetime explicitly (E0303). Lifetimes
    /// appearing nowhere in params are undeclared (E0303).
    fn check_lifetimes(&mut self, f: &FunDecl) {
        // Gather input lifetime slots in order.
        let mut inputs: Vec<Option<String>> = vec![];
        for p in &f.params {
            Self::collect_ref_lts(&p.ty, &mut inputs);
        }
        // Declared = explicit names appearing in inputs.
        let declared: Vec<&str> = inputs
            .iter()
            .filter_map(|l| l.as_deref())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let mut out_lts = vec![];
        Self::collect_ref_lts(&f.ret, &mut out_lts);
        // Every explicit name anywhere must be declared via an input.
        let mut check_name = |lt: &Option<String>, sp: Span| {
            if let Some(n) = lt {
                if !declared.iter().any(|d| *d == n) {
                    self.err(
                        "E0303",
                        sp,
                        format!(
                            "undeclared lifetime `'{}` (lifetimes come from `&` parameters)",
                            n
                        ),
                        None,
                    );
                }
            }
        };
        for p in &f.params {
            Self::each_ref_lt(&p.ty, &mut |lt| check_name(lt, p.span));
        }
        Self::each_ref_lt(&f.ret, &mut |lt| check_name(lt, f.ret_span));
        // Output rule.
        let out_elided = out_lts.iter().any(|l| l.is_none());
        if out_elided {
            // Count DISTINCT input regions: explicit names each count once,
            // and each elided input position counts once.
            let mut distinct = std::collections::HashSet::new();
            let mut n_elided = 0;
            for l in &inputs {
                match l {
                    Some(n) => {
                        distinct.insert(n.clone());
                    }
                    None => n_elided += 1,
                }
            }
            let total = distinct.len() + n_elided;
            if inputs.is_empty() || total != 1 {
                self.err(
                    "E0303",
                    f.ret_span,
                    "elided output lifetime is ambiguous (needs exactly one input lifetime)",
                    Some("name the output lifetime explicitly after one of the inputs".into()),
                );
            }
        }
    }

    /// Top-level borrow lifetimes of a type, in order.
    fn collect_ref_lts(ty: &Ty, out: &mut Vec<Option<String>>) {
        if let Ty::Reference { lifetime, .. } = ty {
            out.push(lifetime.clone());
        }
    }

    /// Visit each top-level borrow lifetime of a type.
    fn each_ref_lt(ty: &Ty, f: &mut impl FnMut(&Option<String>)) {
        if let Ty::Reference { lifetime, .. } = ty {
            f(lifetime);
        }
    }

    /// Whether an expression contains a borrow of a non-`params` root
    /// (dangling if returned).
    fn contains_local_borrow(&self, e: &Expr, params: &std::collections::HashSet<String>) -> bool {
        match &e.kind {
            ExprKind::AddrOf { expr, .. } => match Self::borrow_root(expr) {
                Some(root) => !params.contains(&root),
                None => true, // borrow of temporary: dangling by construction
            },
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Str(_)
            | ExprKind::Var(_) => false,
            ExprKind::Call { args, .. } => {
                args.iter().any(|a| self.contains_local_borrow(a, params))
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.contains_local_borrow(lhs, params) || self.contains_local_borrow(rhs, params)
            }
            ExprKind::Index { base, idx } => {
                self.contains_local_borrow(base, params) || self.contains_local_borrow(idx, params)
            }
            ExprKind::Unary { expr, .. } => self.contains_local_borrow(expr, params),
            ExprKind::Assign { value, .. } => self.contains_local_borrow(value, params),
            ExprKind::If {
                cond,
                then_b,
                else_b,
            } => {
                self.contains_local_borrow(cond, params)
                    || then_b
                        .stmts
                        .iter()
                        .any(|s| self.stmt_has_local_borrow(s, params))
                    || then_b
                        .tail
                        .as_ref()
                        .map(|t| self.contains_local_borrow(t, params))
                        .unwrap_or(false)
                    || else_b
                        .as_ref()
                        .map(|b| {
                            b.stmts
                                .iter()
                                .any(|s| self.stmt_has_local_borrow(s, params))
                                || b.tail
                                    .as_ref()
                                    .map(|t| self.contains_local_borrow(t, params))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false)
            }
            ExprKind::Loop { body } => {
                body.stmts
                    .iter()
                    .any(|s| self.stmt_has_local_borrow(s, params))
                    || body
                        .tail
                        .as_ref()
                        .map(|t| self.contains_local_borrow(t, params))
                        .unwrap_or(false)
            }
            ExprKind::Block(b) => {
                b.stmts
                    .iter()
                    .any(|s| self.stmt_has_local_borrow(s, params))
                    || b.tail
                        .as_ref()
                        .map(|t| self.contains_local_borrow(t, params))
                        .unwrap_or(false)
            }
            ExprKind::Cast { expr, .. } => self.contains_local_borrow(expr, params),
            ExprKind::AsUnit { expr, .. } => self.contains_local_borrow(expr, params),
            ExprKind::StructLit { fields, .. } => fields
                .iter()
                .any(|(_, _, fe)| self.contains_local_borrow(fe, params)),
            ExprKind::Field { base, .. } => self.contains_local_borrow(base, params),
            ExprKind::OptField { base, .. } => self.contains_local_borrow(base, params),
            ExprKind::Variant { args, .. } => {
                args.iter().any(|a| self.contains_local_borrow(a, params))
            }
            ExprKind::VecMacro { elems } => {
                elems.iter().any(|x| self.contains_local_borrow(x, params))
            }
            ExprKind::Method { receiver, args, .. } => {
                self.contains_local_borrow(receiver, params)
                    || args.iter().any(|a| self.contains_local_borrow(a, params))
            }
            ExprKind::Match { scrutinee, arms } => {
                self.contains_local_borrow(scrutinee, params)
                    || arms.iter().any(|a| {
                        a.guard
                            .as_ref()
                            .map(|g| self.contains_local_borrow(g, params))
                            .unwrap_or(false)
                            || a.body
                                .stmts
                                .iter()
                                .any(|s| self.stmt_has_local_borrow(s, params))
                            || a.body
                                .tail
                                .as_ref()
                                .map(|t| self.contains_local_borrow(t, params))
                                .unwrap_or(false)
                    })
            }
            ExprKind::Try { expr } => self.contains_local_borrow(expr, params),
        }
    }

    fn stmt_has_local_borrow(&self, s: &Stmt, params: &std::collections::HashSet<String>) -> bool {
        match s {
            Stmt::Val { init, .. } | Stmt::Var { init, .. } => {
                self.contains_local_borrow(init, params)
            }
            Stmt::Expr { expr, .. } => self.contains_local_borrow(expr, params),
            Stmt::If {
                cond,
                then_b,
                else_b,
                ..
            } => {
                self.contains_local_borrow(cond, params)
                    || then_b
                        .stmts
                        .iter()
                        .any(|x| self.stmt_has_local_borrow(x, params))
                    || else_b
                        .as_ref()
                        .map(|b| {
                            b.stmts
                                .iter()
                                .any(|x| self.stmt_has_local_borrow(x, params))
                        })
                        .unwrap_or(false)
            }
            Stmt::While { cond, body, .. } => {
                self.contains_local_borrow(cond, params)
                    || body
                        .stmts
                        .iter()
                        .any(|x| self.stmt_has_local_borrow(x, params))
            }
            Stmt::Loop { body, .. } => body
                .stmts
                .iter()
                .any(|x| self.stmt_has_local_borrow(x, params)),
            Stmt::For { iter, body, .. } => {
                let h = match iter {
                    ForIter::Range { lo, hi, .. } => {
                        self.contains_local_borrow(lo, params)
                            || self.contains_local_borrow(hi, params)
                    }
                    ForIter::Each { vec } => self.contains_local_borrow(vec, params),
                };
                h || body
                    .stmts
                    .iter()
                    .any(|x| self.stmt_has_local_borrow(x, params))
            }
            Stmt::Break { value, .. } => value
                .as_ref()
                .map(|v| self.contains_local_borrow(v, params))
                .unwrap_or(false),
            Stmt::Continue { .. } => false,
            Stmt::Return { value, .. } => value
                .as_ref()
                .map(|v| self.contains_local_borrow(v, params))
                .unwrap_or(false),
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.contains_local_borrow(scrutinee, params)
                    || arms.iter().any(|a| {
                        a.body
                            .stmts
                            .iter()
                            .any(|x| self.stmt_has_local_borrow(x, params))
                    })
            }
            Stmt::LetElse {
                scrutinee, else_b, ..
            } => {
                self.contains_local_borrow(scrutinee, params)
                    || else_b
                        .stmts
                        .iter()
                        .any(|x| self.stmt_has_local_borrow(x, params))
            }
        }
    }

    fn check_fun(&mut self, f: &FunDecl) {
        self.set_mod(&f.module.clone());
        // main signature validation
        if f.name == "main" {
            if !f.params.is_empty() {
                self.err("E0201", f.span, "`main` must take no parameters", None);
            }
            let main_ok = match &f.ret {
                Ty::Unit | Ty::Int32 => true,
                Ty::Generic { base, args } if base == "Result" && args.len() == 2 => {
                    matches!(args[0], Ty::Int32)
                }
                Ty::Generic { base, args } if base == "Option" && args.len() == 1 => {
                    matches!(args[0], Ty::Int32)
                }
                _ => false,
            };
            if !main_ok && !ty_has_error(&f.ret) {
                self.err(
                    "E0201",
                    f.ret_span,
                    format!("`main` must return `()`, `Int32`, `Result<Int32, E>` or `Option<Int32>`; found {}", f.ret.name()),
                    None,
                );
            }
        }
        self.fn_ret = f.ret.clone();
        self.fn_params = f.params.iter().map(|p| p.name.clone()).collect();
        // Declared return dimension (for `return` conversion checks).
        let ret_dim = self.eval_unit(&f.ret_unit);
        self.fn_ret_dim = ret_dim.clone();
        self.dims.insert(f.id, ret_dim);
        if f.ret_unit.is_some()
            && !matches!(f.ret, Ty::Int32 | Ty::Int64 | Ty::Float64)
            && !ty_has_error(&f.ret)
        {
            self.err(
                "E0201",
                f.ret_span,
                format!("units apply to numeric types, not {}", f.ret.name()),
                None,
            );
        }
        self.check_lifetimes(f);
        self.locals.push(HashMap::new());
        for p in &f.params {
            let pdim = self.eval_unit(&p.unit);
            if p.unit.is_some()
                && !matches!(p.ty, Ty::Int32 | Ty::Int64 | Ty::Float64)
                && !ty_has_error(&p.ty)
            {
                self.err(
                    "E0201",
                    p.span,
                    format!("units apply to numeric types, not {}", p.ty.name()),
                    None,
                );
            }
            self.locals.last_mut().unwrap().insert(
                p.name.clone(),
                Local {
                    ty: p.ty.clone(),
                    mutable: false,
                    dim: pdim.clone(),
                },
            );
            self.param_dim_push(f.id, pdim);
        }
        let (body_ty, flow) = self.check_block(&f.body, &f.ret);
        // Tail/unit coherence: block type must equal declared return, unless the
        // block diverges (ends in unconditional `return`, which coerces to any type).
        // (`break`/`continue` at top level already errored; only Normal must match.)
        if matches!(flow, Flow::Normal)
            && body_ty != f.ret
            && !ty_has_error(&body_ty)
            && !ty_has_error(&f.ret)
        {
            self.err(
                "E0201",
                f.body.span,
                format!(
                    "function `{}` returns {}, but body evaluates to {}",
                    f.name,
                    f.ret.name(),
                    body_ty.name()
                ),
                Some("add `return` with the right type or adjust the return type".into()),
            );
        }
        // Units: tail dimension must match the declared return dimension.
        if matches!(flow, Flow::Normal) && body_ty == f.ret {
            let bdim = self.dim_of(f.body.id);
            if !self.fn_ret_dim.same_dim(&bdim) {
                self.err(
                    "E0201",
                    f.body.span,
                    format!(
                        "function `{}` returns {}, but body evaluates to {}",
                        f.name,
                        self.fn_ret_dim.display(),
                        bdim.display()
                    ),
                    Some("dimensions must agree; convert explicitly".into()),
                );
            }
        }
        self.types.insert(f.id, f.ret.clone());
        // Dangling tails: a function-body tail evaluating to a reference that
        // borrows a non-parameter root escapes its owner (`return &local` is
        // checked in the Return arm; this covers `&local` as the body tail).
        if matches!(f.ret, Ty::Reference { .. }) {
            if let Some(t) = &f.body.tail {
                if self.contains_local_borrow(t, &self.fn_params.clone()) {
                    self.err(
                        "E0303",
                        t.span,
                        "cannot return a reference to a local value (it would dangle)",
                        Some("return an owned value or borrow from a parameter".into()),
                    );
                }
            }
        }
        self.locals.pop();
    }

    /// Returns (block type, exit flow). A block ending in unconditional `return`
    /// with no tail diverges and coerces to any expected type. Blocks ending in
    /// `break`/`continue` evaluate to `()` (they only occur inside loop bodies).
    fn check_block(&mut self, b: &Block, ret_ty: &Ty) -> (Ty, Flow) {
        self.locals.push(HashMap::new());
        let mut flow = Flow::Normal;
        for s in &b.stmts {
            if flow.diverged() {
                self.err(
                    "E0005",
                    stmt_span(s),
                    "unreachable statement after diverging control flow",
                    None,
                );
                break;
            }
            flow = self.check_stmt(s, ret_ty);
        }
        let tail_ty = match &b.tail {
            Some(e) => {
                if flow.diverged() {
                    self.err(
                        "E0005",
                        e.span,
                        "unreachable tail expression after diverging control flow",
                        None,
                    );
                    Ty::Error
                } else {
                    self.check_expr(e)
                }
            }
            None if matches!(flow, Flow::Return) => ret_ty.clone(),
            None => Ty::Unit,
        };
        self.types.insert(b.id, tail_ty.clone());
        let bdim = match &b.tail {
            Some(e) => self.dim_of(e.id),
            None => Dim::dimensionless(),
        };
        self.set_dim(b.id, bdim);
        self.locals.pop();
        let out_flow = if b.tail.is_none() { flow } else { Flow::Normal };
        (tail_ty, out_flow)
    }

    /// Returns how the statement exits (Normal, or diverging Return/Break/Continue).
    fn check_stmt(&mut self, s: &Stmt, ret_ty: &Ty) -> Flow {
        match s {
            Stmt::Val {
                id,
                name,
                name_span,
                ty,
                unit,
                init,
                span,
            } => {
                let aty = ty.as_ref().map(|t| self.resolve_ty(t, *name_span));
                // Annotated positions feed constructor inference via `expected`.
                let saved_exp = self.expected.clone();
                self.expected = aty.clone();
                // Constructors (`None`, `Some`/`Ok`/`Err`) check against the
                // annotation when present.
                let it = match self.ctor_use(init) {
                    Some((cn, cargs)) => {
                        self.check_ctor_call(init.id, init.span, &cn, &cargs, aty.as_ref())
                    }
                    None => self.check_expr(init),
                };
                self.expected = saved_exp;
                if it == Ty::Unit {
                    self.err(
                        "E0201",
                        *span,
                        format!("cannot initialize `{}` with a `()` value", name),
                        Some("Unit carries no data; call the function for effects instead".into()),
                    );
                }
                if it == Ty::Unit {
                    self.err(
                        "E0201",
                        *span,
                        format!("cannot initialize `{}` with a `()` value", name),
                        Some("Unit carries no data; call the function for effects instead".into()),
                    );
                }
                let decl_ty = match aty.as_ref() {
                    Some(t) => {
                        if *t != it && !ty_has_error(t) && !ty_has_error(&it) {
                            self.err(
                                "E0201",
                                *span,
                                format!(
                                    "variable `{}` declared as {}, but initializer is {}",
                                    name,
                                    t.name(),
                                    it.name()
                                ),
                                Some("change the annotation or convert the value".into()),
                            );
                        }
                        t.clone()
                    }
                    None => it.clone(),
                };
                // Record the resolved declared type for codegen (the AST
                // annotation may still hold an unresolved nominal type).
                self.types.insert(*id, decl_ty.clone());
                // Units: declared `@unit` must match the initializer's
                // dimension (literals pin silently; scales convert at
                // codegen); otherwise the binding inherits the
                // initializer's dimension.
                let want = self.eval_unit(unit);
                if unit.is_some()
                    && !matches!(decl_ty, Ty::Int32 | Ty::Int64 | Ty::Float64)
                    && !ty_has_error(&decl_ty)
                {
                    self.err(
                        "E0201",
                        *span,
                        format!("units apply to numeric types, not {}", decl_ty.name()),
                        None,
                    );
                }
                let idim = self.dim_of(init.id);
                let bdim = if unit.is_some() {
                    self.unify_dim(init, &want, *span, &format!("variable `{}`", name));
                    want
                } else {
                    idim
                };
                self.set_dim(*id, bdim.clone());
                if self.locals.last().unwrap().contains_key(name) {
                    self.err(
                        "E0401",
                        *name_span,
                        format!("duplicate local `{}` in the same scope", name),
                        None,
                    );
                } else {
                    if self.locals[..self.locals.len() - 1]
                        .iter()
                        .any(|m| m.contains_key(name))
                    {
                        self.warn(
                            "W1004",
                            *name_span,
                            format!("`{}` shadows an outer binding", name),
                        );
                    }
                    self.locals.last_mut().unwrap().insert(
                        name.clone(),
                        Local {
                            ty: decl_ty.clone(),
                            mutable: false,
                            dim: bdim,
                        },
                    );
                }
                Flow::Normal
            }
            Stmt::Var {
                id,
                name,
                name_span,
                ty,
                unit,
                init,
                span,
            } => {
                let aty = ty.as_ref().map(|t| self.resolve_ty(t, *name_span));
                // Annotated positions feed constructor inference via `expected`.
                let saved_exp = self.expected.clone();
                self.expected = aty.clone();
                // Constructors (`None`, `Some`/`Ok`/`Err`) check against the
                // annotation when present.
                let it = match self.ctor_use(init) {
                    Some((cn, cargs)) => {
                        self.check_ctor_call(init.id, init.span, &cn, &cargs, aty.as_ref())
                    }
                    None => self.check_expr(init),
                };
                self.expected = saved_exp;
                if it == Ty::Unit {
                    self.err(
                        "E0201",
                        *span,
                        format!("cannot initialize `{}` with a `()` value", name),
                        Some("Unit carries no data; call the function for effects instead".into()),
                    );
                }
                if it == Ty::Unit {
                    self.err(
                        "E0201",
                        *span,
                        format!("cannot initialize `{}` with a `()` value", name),
                        Some("Unit carries no data; call the function for effects instead".into()),
                    );
                }
                let decl_ty = match aty.as_ref() {
                    Some(t) => {
                        if *t != it && !ty_has_error(t) && !ty_has_error(&it) {
                            self.err(
                                "E0201",
                                *span,
                                format!(
                                    "variable `{}` declared as {}, but initializer is {}",
                                    name,
                                    t.name(),
                                    it.name()
                                ),
                                None,
                            );
                        }
                        t.clone()
                    }
                    None => it.clone(),
                };
                // Record the resolved declared type for codegen (the AST
                // annotation may still hold an unresolved nominal type).
                self.types.insert(*id, decl_ty.clone());
                // Units: same rule as `val` (see above).
                let want = self.eval_unit(unit);
                if unit.is_some()
                    && !matches!(decl_ty, Ty::Int32 | Ty::Int64 | Ty::Float64)
                    && !ty_has_error(&decl_ty)
                {
                    self.err(
                        "E0201",
                        *span,
                        format!("units apply to numeric types, not {}", decl_ty.name()),
                        None,
                    );
                }
                let idim = self.dim_of(init.id);
                let bdim = if unit.is_some() {
                    self.unify_dim(init, &want, *span, &format!("variable `{}`", name));
                    want
                } else {
                    idim
                };
                self.set_dim(*id, bdim.clone());
                if self.locals.last().unwrap().contains_key(name) {
                    self.err(
                        "E0401",
                        *name_span,
                        format!("duplicate local `{}` in the same scope", name),
                        None,
                    );
                } else {
                    if self.locals[..self.locals.len() - 1]
                        .iter()
                        .any(|m| m.contains_key(name))
                    {
                        self.warn(
                            "W1004",
                            *name_span,
                            format!("`{}` shadows an outer binding", name),
                        );
                    }
                    self.locals.last_mut().unwrap().insert(
                        name.clone(),
                        Local {
                            ty: decl_ty.clone(),
                            mutable: true,
                            dim: bdim,
                        },
                    );
                }
                Flow::Normal
            }
            Stmt::Expr { id, expr, .. } => {
                let t = self.check_expr(expr);
                // Discarded non-unit values are allowed, EXCEPT method results:
                // `v.push(x);` without assignment would silently lose the update.
                if t != Ty::Unit && !ty_has_error(&t) {
                    if let ExprKind::Method { method, .. } = &expr.kind {
                        self.err(
                            "E0201",
                            expr.span,
                            format!(
                                "`{}` returns a value; assign it (e.g. `v = v.{}(...)`)",
                                method, method
                            ),
                            None,
                        );
                    }
                }
                self.types.insert(*id, Ty::Unit);
                // An Expr stmt that is an assignment isn't divergence.
                Flow::Normal
            }
            Stmt::If {
                id,
                cond,
                then_b,
                else_b,
                ..
            } => {
                let ct = self.check_expr(cond);
                if ct != Ty::Bool && !ty_has_error(&ct) {
                    self.err(
                        "E0201",
                        cond.span,
                        format!("`if` condition must be `Bool`, found {}", ct.name()),
                        None,
                    );
                }
                let (t1, d1) = self.check_block(then_b, ret_ty);
                let (t2, _d2) = match else_b {
                    Some(b) => self.check_block(b, ret_ty),
                    None => (Ty::Unit, Flow::Normal),
                };
                // Statement-if: arms must agree only if both diverge-or-unit; be lenient:
                // if no else, then-branch must be Unit (no stray tail value).
                if else_b.is_none()
                    && matches!(d1, Flow::Normal)
                    && t1 != Ty::Unit
                    && !ty_has_error(&t1)
                {
                    self.err(
                        "E0201", then_b.span,
                        "statement `if` without `else` must have `()` branches; add `else` or use statement form",
                        None,
                    );
                }
                if else_b.is_some() && t1 != t2 {
                    // Block-typed statement if: record Unit; mismatch is only an error in expr position.
                    // Keep lenient here (expression form checks strictly).
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::While { id, cond, body, .. } => {
                let ct = self.check_expr(cond);
                if ct != Ty::Bool && !ty_has_error(&ct) {
                    self.err(
                        "E0201",
                        cond.span,
                        format!("`while` condition must be `Bool`, found {}", ct.name()),
                        None,
                    );
                }
                // Like `loop`/`for`: bodies may `break`/`continue`, so push a
                // loop context (`break v` is still rejected below).
                self.loop_stack.push(LoopCtx {
                    is_expr: false,
                    breaks: vec![],
                    plain_break: false,
                });
                let (bt, bflow) = self.check_block(body, ret_ty);
                let ctx = self.loop_stack.pop().expect("loop stack underflow");
                // `break v` in a statement loop discards the value.
                if !ctx.breaks.is_empty() {
                    self.err(
                        "E0201",
                        body.span,
                        "`break` with a value is only allowed in `loop` used as a value",
                        Some("remove the value or use the `loop` as a tail expression".into()),
                    );
                }
                if matches!(bflow, Flow::Normal) && bt != Ty::Unit && !ty_has_error(&bt) {
                    self.err(
                        "E0201",
                        body.span,
                        "`while` body must evaluate to `()`",
                        None,
                    );
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::For {
                id,
                var,
                var_span,
                iter,
                body,
                ..
            } => {
                // Loop-variable type depends on the iteration form.
                let vt = match iter {
                    ForIter::Range { lo, hi, .. } => {
                        let lt = self.check_expr(lo);
                        let rt = self.check_expr(hi);
                        if (lt != rt || !matches!(lt, Ty::Int32 | Ty::Int64))
                            && !ty_has_error(&lt)
                            && !ty_has_error(&rt)
                        {
                            self.err(
                                "E0201",
                                stmt_span(s),
                                format!(
                                    "`for` bounds must both be the same integer type, found {} and {}",
                                    lt.name(),
                                    rt.name()
                                ),
                                Some("only `Int32`/`Int64` ranges are supported".into()),
                            );
                        }
                        lt
                    }
                    ForIter::Each { vec } => {
                        let wt = self.check_expr(vec);
                        match wt {
                            Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                                args[0].clone()
                            }
                            _ => {
                                if !ty_has_error(&wt) {
                                    self.err(
                                        "E0201",
                                        stmt_span(s),
                                        format!(
                                            "`for` without `..` iterates a `Vec<T>`, found {}",
                                            wt.name()
                                        ),
                                        None,
                                    );
                                }
                                if ty_has_error(&wt) {
                                    Ty::Error
                                } else {
                                    Ty::Unit
                                }
                            }
                        }
                    }
                };
                self.loop_stack.push(LoopCtx {
                    is_expr: false,
                    breaks: vec![],
                    plain_break: false,
                });
                self.locals.push(HashMap::new());
                self.locals.last_mut().unwrap().insert(
                    var.clone(),
                    Local {
                        ty: vt.clone(),
                        mutable: false,
                        dim: Dim::dimensionless(),
                    },
                );
                let _ = var_span;
                let (bt, bflow) = self.check_block(body, ret_ty);
                if matches!(bflow, Flow::Normal) && bt != Ty::Unit && !ty_has_error(&bt) {
                    self.err("E0201", body.span, "`for` body must evaluate to `()`", None);
                }
                self.locals.pop();
                self.loop_stack.pop();
                let _ = var_span;
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::Loop { id, body, .. } => {
                self.loop_stack.push(LoopCtx {
                    is_expr: false,
                    breaks: vec![],
                    plain_break: false,
                });
                let (bt, bflow) = self.check_block(body, ret_ty);
                let ctx = self.loop_stack.pop().expect("loop stack underflow");
                // `break v` in a statement loop discards the value.
                if !ctx.breaks.is_empty() {
                    self.err(
                        "E0201",
                        body.span,
                        "`break` with a value is only allowed in `loop` used as a value",
                        Some("remove the value or use the `loop` as a tail expression".into()),
                    );
                }
                if matches!(bflow, Flow::Normal) && bt != Ty::Unit && !ty_has_error(&bt) {
                    self.err(
                        "E0201",
                        body.span,
                        "`loop` body must evaluate to `()`",
                        None,
                    );
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::Match {
                id,
                scrutinee,
                arms,
                span,
                ..
            } => {
                let _ = self.check_match(scrutinee, arms, *span, ret_ty, false);
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::LetElse {
                id,
                scrutinee,
                pat,
                else_b,
                span,
                ..
            } => {
                // `let PAT = expr else DIVERGING` (B3): pattern bindings
                // join the current scope (no push); the `else` block must
                // diverge (Flow-precise, so nested returns count).
                let st = self.check_expr(scrutinee);
                let sdim = self.dim_of(scrutinee.id);
                let _ = self.check_pat(pat, &st, &sdim, *span);
                let (_et, eflow) = self.check_block(else_b, ret_ty);
                if matches!(eflow, Flow::Normal) {
                    self.err(
                        "E0201",
                        *span,
                        "`let ... else` requires a diverging `else` block",
                        Some("end it with `return`, `break`, or `continue`".into()),
                    );
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Normal
            }
            Stmt::Break { id, value, .. } => {
                if self.loop_stack.is_empty() {
                    self.err("E0005", stmt_span(s), "`break` outside of a loop", None);
                    self.types.insert(*id, Ty::Unit);
                    return Flow::Break;
                }
                // Check the value first (ends any borrow of the loop stack).
                let vt = value.as_ref().map(|v| self.check_expr(v));
                let ctx = self.loop_stack.last_mut().expect("checked above");
                match vt {
                    None => ctx.plain_break = true,
                    Some(t) => {
                        if !ctx.is_expr {
                            self.err(
                                "E0201",
                                stmt_span(s),
                                "`break` with a value is only allowed in `loop` used as a value",
                                None,
                            );
                        } else {
                            ctx.breaks.push(t);
                        }
                    }
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Break
            }
            Stmt::Continue { id, .. } => {
                if self.loop_stack.is_empty() {
                    self.err("E0005", stmt_span(s), "`continue` outside of a loop", None);
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Continue
            }
            Stmt::Return { id, value, .. } => {
                let saved_exp = self.expected.clone();
                self.expected = Some(ret_ty.clone());
                let vt = match value {
                    Some(e) => match self.ctor_use(e) {
                        Some((cn, cargs)) => {
                            let exp = Some(ret_ty.clone());
                            self.check_ctor_call(e.id, e.span, &cn, &cargs, exp.as_ref())
                        }
                        None => self.check_expr(e),
                    },
                    None => Ty::Unit,
                };
                self.expected = saved_exp;
                if vt != *ret_ty && !ty_has_error(&vt) && !ty_has_error(ret_ty) {
                    self.err(
                        "E0201",
                        stmt_span(s),
                        format!(
                            "`return` value is {}, but function returns {}",
                            vt.name(),
                            ret_ty.name()
                        ),
                        None,
                    );
                }
                // Units: returned value must share the declared dimension
                // (literals pin; scales convert silently at codegen).
                if let Some(v) = value {
                    self.unify_dim(v, &self.fn_ret_dim.clone(), stmt_span(s), "`return` value");
                }
                // Dangling references cannot be returned: any borrow of a
                // non-parameter root inside the value escapes its owner.
                if matches!(vt, Ty::Reference { .. }) {
                    if let Some(v) = value {
                        if self.contains_local_borrow(v, &self.fn_params.clone()) {
                            self.err(
                                "E0303",
                                stmt_span(s),
                                "cannot return a reference to a local value (it would dangle)",
                                Some("return an owned value or borrow from a parameter".into()),
                            );
                        }
                    }
                }
                self.types.insert(*id, Ty::Unit);
                Flow::Return
            }
        }
    }

    fn check_expr(&mut self, e: &Expr) -> Ty {
        let ty = match &e.kind {
            ExprKind::Int(v) => {
                if *v > i32::MAX as i64 || *v < i32::MIN as i64 {
                    // Unconstrained literal wider than Int32: infer Int64.
                    Ty::Int64
                } else {
                    Ty::Int32
                }
            }
            ExprKind::Float(_) => Ty::Float64,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Str(_) => Ty::Str,
            ExprKind::Var(name) => {
                match self.lookup(name) {
                    Some(l) => {
                        let (ty, dim) = (l.ty.clone(), l.dim.clone());
                        self.set_dim(e.id, dim);
                        ty
                    }
                    None if name == "print" || name == "println" => {
                        self.err(
                            "E0401",
                            e.span,
                            format!("`{}` used as a value; it is only callable", name),
                            None,
                        );
                        Ty::Error
                    }
                    None => {
                        // function names are not first-class values in 0.1.0
                        if self.resolve_fun(name).is_some() {
                            self.err("E0800", e.span, format!("function `{}` used as a value (first-class functions deferred)", name), None);
                        } else {
                            self.err(
                                "E0401",
                                e.span,
                                format!("unknown variable `{}`", name),
                                Some("declare it with `val`/`var` or check the spelling".into()),
                            );
                        }
                        Ty::Error
                    }
                }
            }
            ExprKind::Call { name, args } => {
                let t = self.check_call(e.id, e.span, name, args);
                self.types.insert(e.id, t.clone());
                t
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                let ld = self.dim_of(lhs.id);
                let rd = self.dim_of(rhs.id);
                self.check_binary(BinCheck {
                    id: e.id,
                    span: e.span,
                    op: *op,
                    lt: &lt,
                    rt: &rt,
                    ld: &ld,
                    rd: &rd,
                    lhs: Some(lhs.as_ref()),
                    rhs: Some(rhs.as_ref()),
                })
            }
            ExprKind::Unary { op, expr } => {
                let t = self.check_expr(expr);
                if ty_has_error(&t) {
                    return Ty::Error;
                }
                // Negation and dereference preserve dimensions.
                let d = self.dim_of(expr.id);
                match op {
                    UnOp::Neg => match t {
                        Ty::Int32 | Ty::Int64 | Ty::Float64 => {
                            self.set_dim(e.id, d);
                            t
                        }
                        _ => {
                            self.err(
                                "E0201",
                                e.span,
                                format!("unary `-` needs a number, found {}", t.name()),
                                None,
                            );
                            Ty::Int32
                        }
                    },
                    UnOp::Not => {
                        if t != Ty::Bool {
                            self.err(
                                "E0201",
                                e.span,
                                format!("`!` needs `Bool`, found {}", t.name()),
                                None,
                            );
                        }
                        Ty::Bool
                    }
                    UnOp::Deref => match t {
                        Ty::Reference { inner, .. } => {
                            let d = self.dim_of(expr.id);
                            self.set_dim(e.id, d);
                            *inner.clone()
                        }
                        _ => {
                            if ty_has_error(&t) {
                                return Ty::Error;
                            }
                            self.err(
                                "E0201",
                                e.span,
                                format!("cannot dereference {}", t.name()),
                                None,
                            );
                            Ty::Error
                        }
                    },
                }
            }
            ExprKind::Assign { target, op, value } => {
                let vt = self.check_expr(value);
                // Only an outermost index carries a bounds check: an index
                // under Field/Deref (e.g. `(*a)[t].red = v`) would reach
                // codegen addressing without one, so reject it here with
                // the same E0201 as index-under-index (bind the element
                // first). Index-under-Index is rejected inside check_place.
                if !matches!(target, Place::Index { .. }) && Self::place_has_index(target) {
                    self.err(
                        "E0201",
                        target.span(),
                        "nested index assignment is not supported (bind the inner element first)",
                        None,
                    );
                    return self.insert(e.id, Ty::Unit);
                }
                // Resolve the place: (place type, mutable?).
                match self.check_place(target) {
                    None => Ty::Unit,
                    Some((pt, mutable, pdim)) => {
                        if !mutable {
                            self.err(
                                "E0205",
                                e.span,
                                "cannot assign through this place (immutable binding or shared reference)",
                                Some("use `var` bindings and `&mut` for mutation".into()),
                            );
                        }
                        // Units: `Set` unifies the value with the place
                        // (literals pin; scales convert at codegen).
                        // Compound ops check dimensions per-operator below.
                        if *op == AssignOp::Set {
                            self.unify_dim(value, &pdim, e.span, "cannot assign");
                        }
                        self.set_dim(e.id, pdim.clone());
                        if *op == AssignOp::Set {
                            if pt != vt && !ty_has_error(&pt) && !ty_has_error(&vt) {
                                self.err(
                                    "E0201",
                                    e.span,
                                    format!(
                                        "cannot assign {} to place of type {}",
                                        vt.name(),
                                        pt.name()
                                    ),
                                    None,
                                );
                            }
                        } else {
                            // compound: numeric only, same-type, same dimension.
                            // `*=`/`/=` compose dimensions, which would change
                            // the place's dimension — so factors must be
                            // dimensionless (codegen cannot retype the slot).
                            if matches!(op, AssignOp::Mul | AssignOp::Div) {
                                let vdim = self.dim_of(value.id);
                                if !vdim.is_dimensionless() {
                                    self.err(
                                        "E0201",
                                        e.span,
                                        format!(
                                            "compound `{}` needs a dimensionless factor, found {}",
                                            if *op == AssignOp::Mul { "*=" } else { "/=" },
                                            vdim.display()
                                        ),
                                        Some("dimensions must agree; convert explicitly".into()),
                                    );
                                }
                            }
                            let vdim = self.dim_of(value.id);
                            let res = self.check_binary(BinCheck {
                                id: e.id,
                                span: e.span,
                                op: match op {
                                    AssignOp::Add => BinOp::Add,
                                    AssignOp::Sub => BinOp::Sub,
                                    AssignOp::Mul => BinOp::Mul,
                                    AssignOp::Div => BinOp::Div,
                                    _ => BinOp::Rem,
                                },
                                lt: &pt,
                                rt: &vt,
                                ld: &pdim,
                                rd: &vdim,
                                lhs: None,
                                rhs: Some(value.as_ref()),
                            });
                            if res != pt && !ty_has_error(&res) && !ty_has_error(&pt) {
                                self.err(
                                    "E0201",
                                    e.span,
                                    "compound assignment changes place type",
                                    None,
                                );
                            }
                            // The place keeps its dimension regardless of what
                            // the operator composed (Mul/Div factors above are
                            // dimensionless, so this is identity for them).
                            self.set_dim(e.id, pdim.clone());
                        }
                        Ty::Unit
                    }
                }
            }
            ExprKind::If {
                cond,
                then_b,
                else_b,
            } => {
                let ct = self.check_expr(cond);
                if ct != Ty::Bool && !ty_has_error(&ct) {
                    self.err(
                        "E0201",
                        cond.span,
                        format!("`if` condition must be `Bool`, found {}", ct.name()),
                        None,
                    );
                }
                // Expression-if: else required.
                let els = match else_b {
                    Some(b) => b,
                    None => {
                        self.err(
                            "E0201",
                            e.span,
                            "`if` used as a value requires `else`",
                            Some("add an `else` branch".into()),
                        );
                        // still check then-branch for error cascade reduction
                        self.check_block(then_b, &Ty::Unit);
                        return self.insert(e.id, Ty::Error);
                    }
                };
                // Check arms in a scratch Unit-return context? Arms must not contain bare `return`
                // mismatches — reuse enclosing? We don't track enclosing ret here; approximate:
                let (t1, d1) = self.check_block_expr(then_b);
                let (t2, d2) = self.check_block_expr(els);
                let dim1 = self.dim_of(then_b.id);
                let dim2 = self.dim_of(els.id);
                match (d1, d2) {
                    (true, true) => Ty::Unit, // both diverge: whole if diverges (Unit here)
                    (true, false) => {
                        self.set_dim(e.id, dim2.clone());
                        t2
                    }
                    (false, true) => {
                        self.set_dim(e.id, dim1.clone());
                        t1
                    }
                    (false, false) => {
                        if t1 != t2 && !ty_has_error(&t1) && !ty_has_error(&t2) {
                            self.err(
                                "E0201",
                                e.span,
                                format!("`if` branches disagree: {} vs {}", t1.name(), t2.name()),
                                None,
                            );
                            Ty::Error
                        } else if ty_has_error(&t1) || ty_has_error(&t2) {
                            Ty::Error
                        } else {
                            // Units: branches must share the dimension
                            // (literals pin); the first arm's scale wins
                            // (codegen converts).
                            let dim1 = self.dim_of(then_b.id);
                            match &els.tail {
                                Some(t) => self.unify_dim(t, &dim1, e.span, "`if` branch"),
                                None => {
                                    if !dim1.is_dimensionless() {
                                        self.err(
                                            "E0201",
                                            e.span,
                                            format!(
                                                "`if` branches disagree in dimension: {} vs 1",
                                                dim1.display()
                                            ),
                                            Some(
                                                "dimensions must agree; convert explicitly".into(),
                                            ),
                                        );
                                    }
                                }
                            }
                            self.set_dim(e.id, dim1.clone());
                            t1
                        }
                    }
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                let (t, d) = self.check_match(scrutinee, arms, e.span, &self.fn_ret.clone(), true);
                self.set_dim(e.id, d);
                t
            }
            ExprKind::Block(b) => {
                // Desugared statement sequence (A3 multi-`println`, chained
                // comparisons): check like a value block, take its type.
                let (t, _) = self.check_block_expr(b);
                self.set_dim(e.id, self.dim_of(b.id));
                t
            }
            ExprKind::Loop { body } => {
                self.loop_stack.push(LoopCtx {
                    is_expr: true,
                    breaks: vec![],
                    plain_break: false,
                });
                let rt = self.fn_ret.clone();
                let (bt, bflow) = self.check_block(body, &rt);
                let ctx = self.loop_stack.pop().expect("loop stack underflow");
                if matches!(bflow, Flow::Normal) && bt != Ty::Unit && !ty_has_error(&bt) {
                    self.err(
                        "E0201",
                        body.span,
                        "`loop` body must evaluate to `()`",
                        None,
                    );
                }
                // All `break v` values must agree; no `break v` means the loop
                // diverges (runs forever) — type it as Unit here. Mixing plain
                // `break` with `break v` would leave the result uninitialized.
                if ctx.plain_break && !ctx.breaks.is_empty() {
                    self.err(
                        "E0201",
                        e.span,
                        "cannot mix plain `break` with `break` carrying a value in a `loop` value",
                        None,
                    );
                    return self.insert(e.id, Ty::Error);
                }
                let mut it = ctx.breaks.iter();
                match it.next() {
                    None => Ty::Unit,
                    Some(first) => {
                        let mut ty = first.clone();
                        for b in it {
                            if *b != ty {
                                if ty_has_error(&ty) || ty_has_error(b) {
                                    ty = Ty::Error;
                                    break;
                                }
                                self.err(
                                    "E0201",
                                    e.span,
                                    format!(
                                        "`break` values disagree: {} vs {}",
                                        ty.name(),
                                        b.name()
                                    ),
                                    None,
                                );
                                ty = Ty::Error;
                                break;
                            }
                        }
                        ty
                    }
                }
            }
            ExprKind::OptField {
                base,
                field,
                field_span,
                ..
            } => {
                let bt = self.check_expr(base);
                if ty_has_error(&bt) {
                    return self.insert(e.id, Ty::Error);
                }
                let payload = match &bt {
                    Ty::Generic { base, args } if base == "Option" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        self.err(
                            "E0201",
                            *field_span,
                            format!("`?.` needs an `Option<T>` base, found {}", bt.name()),
                            None,
                        );
                        return self.insert(e.id, Ty::Error);
                    }
                };
                match payload {
                    Ty::Struct(sn) => match self.structs.get(&sn).cloned() {
                        Some(def) => match def.fields.iter().find(|(f, _)| f == field) {
                            Some((_, fty)) => {
                                let key = Self::inst_key("Option", std::slice::from_ref(fty));
                                self.generic_insts
                                    .insert(key, ("Option".to_string(), vec![fty.clone()]));
                                Ty::Generic {
                                    base: "Option".to_string(),
                                    args: vec![fty.clone()],
                                }
                            }
                            None => {
                                self.err(
                                    "E0401",
                                    *field_span,
                                    format!("unknown field `{}.{}`", sn, field),
                                    None,
                                );
                                Ty::Error
                            }
                        },
                        None => Ty::Unit,
                    },
                    other => {
                        if ty_has_error(&other) {
                            return Ty::Error;
                        }
                        self.err(
                            "E0201",
                            *field_span,
                            format!("`?.` payload {} has no fields", other.name()),
                            None,
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::Try { expr, .. } => {
                // T-Prop (0.1.0): operand `Result<T, E>` requires enclosing
                // `Result<U, E>` (identical error type; `From` deferred);
                // operand `Option<T>` requires enclosing `Option<U>`.
                // Success type is unconstrained (flows into the body).
                let ot = self.check_expr(expr);
                if ty_has_error(&ot) {
                    return Ty::Error;
                }
                let fret = self.fn_ret.clone();
                match (&ot, &fret) {
                    (
                        Ty::Generic {
                            base: ob,
                            args: oargs,
                        },
                        Ty::Generic { base: fb, args: _ },
                    ) if ob == "Result" && fb == "Result" && oargs.len() == 2 => {
                        // Error-type identity (no From conversions yet).
                        if let Ty::Generic { args: fargs, .. } = &fret {
                            if fargs.len() == 2 && fargs[1] != oargs[1] && !ty_has_error(&fargs[1])
                            {
                                self.err(
                                    "E0702",
                                    e.span,
                                    format!(
                                        "`?` needs identical error types (found {} vs {}); error conversion is deferred",
                                        oargs[1].name(),
                                        fargs[1].name()
                                    ),
                                    None,
                                );
                            }
                        }
                        oargs[0].clone()
                    }
                    (
                        Ty::Generic {
                            base: ob,
                            args: oargs,
                        },
                        Ty::Generic { base: fb, args: _ },
                    ) if ob == "Option" && fb == "Option" && oargs.len() == 1 => oargs[0].clone(),
                    _ => {
                        if ty_has_error(&fret) {
                            return Ty::Error;
                        }
                        self.err(
                            "E0702",
                            e.span,
                            format!(
                                "`?` on {} needs an enclosing function returning `Result`/`Option` (found {})",
                                ot.name(),
                                fret.name()
                            ),
                            Some("return a `Result`/`Option` from this function to use `?`".into()),
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::VecMacro { elems } => {
                // Element type from the annotation when present, else inferred
                // from the first element (empty literals need annotation).
                let exp_elem: Option<Ty> = match &self.expected {
                    Some(Ty::Generic { base, args }) if base == "Vec" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    _ => None,
                };
                if elems.is_empty() {
                    match exp_elem {
                        Some(t) => {
                            let vt = Ty::Generic {
                                base: "Vec".to_string(),
                                args: vec![t],
                            };
                            self.record_ty(&vt);
                            vt
                        }
                        None => {
                            self.err(
                                "E0201",
                                e.span,
                                "`vec![]` needs an expected `Vec<T>` type; annotate the binding",
                                None,
                            );
                            Ty::Error
                        }
                    }
                } else {
                    // Seed the expected element type from the annotation,
                    // else from the first element, so bare constructors
                    // (`None`, `Missing`) resolve against their siblings:
                    // `vec![Some(1), None]`, `vec![Present(1.0), Missing]`.
                    let mut seed: Option<Ty> = exp_elem.clone();
                    let mut t0_opt: Option<Ty> = None;
                    for (idx, el) in elems.iter().enumerate() {
                        // Qualified variants (`Missing::Missing`) read the
                        // ambient expectation: seed it when known.
                        let saved_exp = self.expected.clone();
                        if idx > 0 && self.expected.is_none() {
                            self.expected = seed.clone();
                        }
                        let t = match self.ctor_use(el) {
                            Some((cn, cargs)) => {
                                let exp = seed.as_ref().or(t0_opt.as_ref());
                                self.check_ctor_call(el.id, el.span, &cn, &cargs, exp)
                            }
                            None => self.check_expr(el),
                        };
                        self.expected = saved_exp;
                        if seed.is_none() {
                            seed = Some(t.clone());
                        }
                        match &t0_opt {
                            None => t0_opt = Some(t),
                            Some(t0) => {
                                if t != *t0 && !ty_has_error(t0) && !ty_has_error(&t) {
                                    self.err(
                                        "E0201",
                                        el.span,
                                        format!(
                                            "`vec!` elements must share one type (found {} vs {})",
                                            t0.name(),
                                            t.name()
                                        ),
                                        None,
                                    );
                                }
                            }
                        }
                    }
                    let t0 = t0_opt.unwrap_or(Ty::Unit);
                    let vt = if let Some(exp) = exp_elem {
                        if exp != t0 && !ty_has_error(&exp) && !ty_has_error(&t0) {
                            self.err(
                                "E0201",
                                e.span,
                                format!("expected Vec<{}>, found Vec<{}>", exp.name(), t0.name()),
                                None,
                            );
                        }
                        Ty::Generic {
                            base: "Vec".to_string(),
                            args: vec![exp],
                        }
                    } else {
                        Ty::Generic {
                            base: "Vec".to_string(),
                            args: vec![t0],
                        }
                    };
                    self.record_ty(&vt);
                    vt
                }
            }
            ExprKind::Method {
                receiver,
                method,
                method_span,
                args: call_args,
            } => {
                let rt = self.check_expr(receiver);
                match rt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        let elem = args[0].clone();
                        let t =
                            self.check_vec_method(e.span, &elem, method, *method_span, call_args);
                        self.record_ty(&t);
                        t
                    }
                    Ty::Generic { base, args } if base == "Mat" && args.is_empty() => {
                        let t = self.check_mat_method(e.span, method, *method_span, call_args);
                        self.record_ty(&t);
                        t
                    }
                    Ty::Generic { base, args } if base == "Factor" && args.is_empty() => {
                        let t = self.check_factor_method(e.span, method, *method_span, call_args);
                        self.record_ty(&t);
                        t
                    }
                    Ty::Str => {
                        let t = self.check_str_method(e.span, method, *method_span, call_args);
                        self.record_ty(&t);
                        t
                    }
                    Ty::Generic { base, args } if base == "Rng" && args.is_empty() => {
                        self.check_rng_method(e.span, method, *method_span, call_args)
                    }
                    _ => {
                        if ty_has_error(&rt) {
                            return Ty::Error;
                        }
                        // UFCS (C2): `x.f(args)` falls back to free
                        // `f(x, args)` when the receiver type has no methods.
                        // Real methods always win (earlier arms).
                        let ufcs = self
                            .resolve_fun(method)
                            .and_then(|fid| self.sigs.get(&fid).cloned())
                            .filter(|(pt, _)| pt.first().is_some_and(|p| p == &rt));
                        match ufcs {
                            Some(_) => {
                                let mut uargs: Vec<Expr> = Vec::with_capacity(call_args.len() + 1);
                                uargs.push((**receiver).clone());
                                uargs.extend(call_args.iter().cloned());
                                let t = self.check_call(e.id, e.span, method, &uargs);
                                self.record_ty(&t);
                                t
                            }
                            None => {
                                self.err(
                                    "E0401",
                                    *method_span,
                                    format!(
                                        "type {} has no methods (only `Vec`, `Mat`, `Factor`, `String` and `Rng` have methods)",
                                        rt.name()
                                    ),
                                    None,
                                );
                                Ty::Error
                            }
                        }
                    }
                }
            }
            ExprKind::Cast { expr, target, .. } => {
                let st = self.check_expr(expr);
                if ty_has_error(&st) {
                    return Ty::Error;
                }
                if !can_cast(&st, target) {
                    self.err(
                        "E0201",
                        e.span,
                        format!("cannot cast {} to {}", st.name(), target.name()),
                        Some("supported: int<->int, int<->Float64 (float->int saturates)".into()),
                    );
                }
                if st == *target {
                    self.warn(
                        "W1003",
                        e.span,
                        format!("needless `as` (already {})", st.name()),
                    );
                }
                // Casts change representation, never dimension.
                let d = self.dim_of(expr.id);
                self.set_dim(e.id, d);
                target.clone()
            }
            ExprKind::AsUnit { expr, unit } => {
                // `x as @unit`: numeric only; dimensionless operands adopt
                // the dimension, same-vector operands convert scales,
                // anything else is E0201. No runtime effect beyond scale.
                let t = self.check_expr(expr);
                if ty_has_error(&t) {
                    return self.insert(e.id, Ty::Error);
                }
                match t {
                    Ty::Int32 | Ty::Int64 | Ty::Float64 => {}
                    _ => {
                        self.err(
                            "E0201",
                            e.span,
                            format!("`as @unit` needs a number, found {}", t.name()),
                            None,
                        );
                        return self.insert(e.id, Ty::Error);
                    }
                }
                let want = self.eval_unit_expr(unit);
                let have = self.dim_of(expr.id);
                if !have.is_dimensionless() && !want.same_dim(&have) {
                    self.err(
                        "E0201",
                        e.span,
                        format!("cannot interpret {} as {}", have.display(), want.display()),
                        Some("dimensions must agree; convert explicitly".into()),
                    );
                }
                self.set_dim(e.id, want);
                t
            }
            ExprKind::StructLit { name, fields, .. } => {
                let qname = self.resolve_type(name);
                let def = match qname.as_ref().and_then(|q| self.structs.get(q)).cloned() {
                    Some(d) => d,
                    None => {
                        if self
                            .resolve_type(name)
                            .as_ref()
                            .is_some_and(|q| self.enums.contains_key(q))
                        {
                            self.err(
                                "E0201",
                                e.span,
                                format!("`{}` is an enum; use `{}::Variant(..)`", name, name),
                                None,
                            );
                        } else if self.resolve_fun(name).is_some() {
                            self.err(
                                "E0201",
                                e.span,
                                format!("`{}` is a function, not a struct", name),
                                None,
                            );
                        } else {
                            self.err("E0401", e.span, format!("unknown type `{}`", name), None);
                        }
                        return self.insert(e.id, Ty::Error);
                    }
                };
                if def.is_tuple {
                    self.err(
                        "E0201",
                        e.span,
                        format!(
                            "`{}` is a tuple struct; construct it with `{}(..)`",
                            name, name
                        ),
                        None,
                    );
                    return self.insert(
                        e.id,
                        Ty::Struct(qname.clone().unwrap_or_else(|| name.clone())),
                    );
                }
                if fields.is_empty() && def.fields.is_empty() {
                    // Unit struct `Empty {}`.
                    return self.insert(
                        e.id,
                        Ty::Struct(qname.clone().unwrap_or_else(|| name.clone())),
                    );
                }
                // Duplicate + unknown + missing checks.
                let mut seen = std::collections::HashSet::new();
                for (fname, fspan, _) in fields {
                    if !seen.insert(fname.clone()) {
                        self.err(
                            "E0401",
                            *fspan,
                            format!("duplicate field `{}`", fname),
                            None,
                        );
                    }
                    if !def.fields.iter().any(|(f, _)| f == fname) {
                        let avail: Vec<&str> = def.fields.iter().map(|(f, _)| f.as_str()).collect();
                        self.err(
                            "E0401",
                            *fspan,
                            format!(
                                "unknown field `{}.{}` (available: {})",
                                name,
                                fname,
                                avail.join(", ")
                            ),
                            None,
                        );
                    }
                }
                for (fname, fty) in &def.fields {
                    match fields.iter().find(|(f, _, _)| f == fname) {
                        None => {
                            self.err(
                                "E0201",
                                e.span,
                                format!("missing field `{}.{}`", name, fname),
                                None,
                            );
                        }
                        Some((_, _, fexpr)) => {
                            let at = self.check_expr(fexpr);
                            if at != *fty && !ty_has_error(&at) && !ty_has_error(fty) {
                                self.err(
                                    "E0201",
                                    fexpr.span,
                                    format!(
                                        "field `{}` is {}, but value is {}",
                                        fname,
                                        fty.name(),
                                        at.name()
                                    ),
                                    None,
                                );
                            }
                        }
                    }
                }
                self.insert(e.id, Ty::Struct(qname.unwrap_or_else(|| name.clone())))
            }
            ExprKind::Field {
                base,
                field,
                field_span,
                ..
            } => {
                let bt = self.check_expr(base);
                match bt {
                    Ty::Struct(sn) => match self.structs.get(&sn).cloned() {
                        Some(def) => match def.fields.iter().find(|(f, _)| f == field) {
                            Some((_, fty)) => fty.clone(),
                            None => {
                                let avail: Vec<&str> =
                                    def.fields.iter().map(|(f, _)| f.as_str()).collect();
                                self.err(
                                    "E0401",
                                    *field_span,
                                    format!(
                                        "unknown field `{}.{}` (available: {})",
                                        sn,
                                        field,
                                        avail.join(", ")
                                    ),
                                    None,
                                );
                                Ty::Error
                            }
                        },
                        None => Ty::Unit,
                    },
                    Ty::Enum(en) => {
                        self.err(
                            "E0201",
                            *field_span,
                            format!("enum `{}` has no fields; match on its variants instead", en),
                            None,
                        );
                        Ty::Error
                    }
                    Ty::Reference { inner, .. } => {
                        // Auto-deref for field access through references.
                        match inner.as_ref() {
                            Ty::Struct(sn) => match self.structs.get(sn).cloned() {
                                Some(def) => match def.fields.iter().find(|(f, _)| f == field) {
                                    Some((_, fty)) => fty.clone(),
                                    None => {
                                        self.err(
                                            "E0401",
                                            *field_span,
                                            format!("unknown field `{}.{}`", sn, field),
                                            None,
                                        );
                                        Ty::Error
                                    }
                                },
                                None => Ty::Unit,
                            },
                            _ => {
                                self.err(
                                    "E0201",
                                    *field_span,
                                    "cannot access fields through this reference",
                                    None,
                                );
                                Ty::Error
                            }
                        }
                    }
                    other => {
                        if ty_has_error(&other) {
                            return Ty::Error;
                        }
                        self.err(
                            "E0201",
                            *field_span,
                            format!("type {} has no fields", other.name()),
                            None,
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::Index { base, idx } => {
                let bt = self.check_expr(base);
                let it = self.check_expr(idx);
                if ty_has_error(&bt) || ty_has_error(&it) {
                    return Ty::Error;
                }
                if !matches!(it, Ty::Int32 | Ty::Int64) {
                    self.err(
                        "E0201",
                        idx.span,
                        format!("vector index must be Int32/Int64, found {}", it.name()),
                        None,
                    );
                }
                match bt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        let t = args[0].clone();
                        self.record_ty(&t);
                        t
                    }
                    _ => {
                        self.err(
                            "E0401",
                            e.span,
                            format!(
                                "type {} cannot be indexed (only `Vec` supports `v[i]`)",
                                bt.name()
                            ),
                            None,
                        );
                        Ty::Error
                    }
                }
            }
            ExprKind::AddrOf { expr, mutable, .. } => {
                // Borrows need variable-rooted places (no temporaries: they
                // would dangle). Fresh elided lifetime per borrow.
                match Self::borrow_root(expr) {
                    None => {
                        self.err(
                            "E0303",
                            e.span,
                            "cannot borrow a temporary value (it would dangle)",
                            Some("bind it with `val`/`var` first".into()),
                        );
                        Ty::Error
                    }
                    Some(root) => {
                        if *mutable {
                            // `&mut` needs a mutable (`var`) root path.
                            let ok = match self.lookup(&root).cloned() {
                                Some(l) => l.mutable,
                                None => false,
                            };
                            if !ok {
                                self.err(
                                    "E0205",
                                    e.span,
                                    format!("cannot mutably borrow immutable `{}`", root),
                                    Some("declare it with `var`".into()),
                                );
                                return self.insert(e.id, Ty::Error);
                            }
                        }
                        let inner = self.check_expr(expr);
                        Ty::Reference {
                            lifetime: None,
                            mutable: *mutable,
                            inner: Box::new(inner),
                        }
                    }
                }
            }
            ExprKind::Variant {
                ty,
                variant,
                var_span,
                args,
                ..
            } => {
                // Builtin `Option::`/`Result::`/`Missing::` construction. Known sides come
                // from the arguments; unknown sides (`None`, bare `Ok`/`Err`,
                // `Missing::Missing`) come from the expected type; otherwise
                // E0201 (never guess).
                if ty == "Option" || ty == "Result" || ty == "Missing" {
                    let exp = self.expected.clone();
                    let t = self.check_builtin_variant(
                        e.span,
                        ty,
                        variant,
                        *var_span,
                        args,
                        exp.as_ref(),
                    );
                    return self.insert(e.id, t);
                }
                let qtype = self.resolve_type(ty);
                let def = match qtype.as_ref().and_then(|q| self.enums.get(q)).cloned() {
                    Some(d) => d,
                    None => {
                        if qtype.as_ref().is_some_and(|q| self.structs.contains_key(q)) {
                            self.err(
                                "E0201",
                                e.span,
                                format!("`{}` is a struct; use `{} {{ ... }}`", ty, ty),
                                None,
                            );
                        } else {
                            self.err("E0401", e.span, format!("unknown type `{}`", ty), None);
                        }
                        return self.insert(e.id, Ty::Error);
                    }
                };
                match def.variants.iter().find(|(v, _)| v == variant) {
                    None => {
                        let avail: Vec<&str> =
                            def.variants.iter().map(|(v, _)| v.as_str()).collect();
                        self.err(
                            "E0401",
                            *var_span,
                            format!(
                                "unknown variant `{}::{}` (available: {})",
                                ty,
                                variant,
                                avail.join(", ")
                            ),
                            None,
                        );
                        self.insert(e.id, Ty::Enum(qtype.unwrap_or_else(|| ty.clone())))
                    }
                    Some((_, payload)) => {
                        let what = format!("{}::{}", ty, variant);
                        let fields: Vec<(String, Ty)> = payload
                            .iter()
                            .enumerate()
                            .map(|(i, t)| (format!("_{}", i), t.clone()))
                            .collect();
                        let ret = Ty::Enum(qtype.unwrap_or_else(|| ty.clone()));
                        self.check_tuple_ctor(e.span, &what, &fields, args, ret)
                    }
                }
            }
        };
        self.insert(e.id, ty)
    }

    fn check_block_expr(&mut self, b: &Block) -> (Ty, bool) {
        self.locals.push(HashMap::new());
        let mut diverged = false;
        for s in &b.stmts {
            // `return` inside expression-if arms: check loosely (must match Unit ctx is wrong...).
            // Simplification: reject `return` inside value-position blocks in 0.1.0.
            if matches!(s, Stmt::Return { .. }) {
                self.err("E0800", stmt_span(s), "`return` inside an `if` value-branch is not yet supported; restructure with statements", None);
                diverged = true;
            } else if stmt_tree_has_return(s) {
                // Nested `return` (e.g. inside a nested `if` in the arm) must
                // not fall through to the dummy-Unit check below: that yields
                // a bogus "`return` value is T, but function returns ()".
                self.err("E0800", stmt_span(s), "`return` inside an `if`/`match` value-branch is not yet supported; restructure with statements", None);
                diverged = true;
            } else {
                // check with dummy Unit return type (return already rejected)
                let dummy = Ty::Unit;
                self.check_stmt_noret(s, &dummy);
            }
        }
        let t = match &b.tail {
            Some(e) => self.check_expr(e),
            None if diverged => Ty::Unit,
            None => Ty::Unit,
        };
        self.types.insert(b.id, t.clone());
        // Block dimension follows its tail (for `if`/`match` arm agreement).
        let bd = match &b.tail {
            Some(e) => self.dim_of(e.id),
            None => Dim::dimensionless(),
        };
        self.set_dim(b.id, bd);
        self.locals.pop();
        (t, diverged && b.tail.is_none())
    }

    fn check_stmt_noret(&mut self, s: &Stmt, dummy: &Ty) {
        match s {
            Stmt::Return { .. } => unreachable!(),
            Stmt::Break { .. } | Stmt::Continue { .. } => {
                // Value-position arms can sit inside a loop (`while(c){ val x = if(d){break}else{2} }`).
                if self.loop_stack.is_empty() {
                    self.err(
                        "E0005",
                        stmt_span(s),
                        "`break`/`continue` outside of a loop",
                        None,
                    );
                } else {
                    let _ = self.check_stmt(s, dummy);
                }
            }
            _ => {
                let flow = self.check_stmt(s, dummy);
                debug_assert!(matches!(flow, Flow::Normal));
            }
        }
    }

    /// Whether `name` is shadowed by a user binding (locals only; a `fun`
    /// with a ctor name is itself an error magnet we leave alone).
    fn shadowed(&self, name: &str) -> bool {
        self.locals.iter().rev().any(|m| m.contains_key(name))
    }

    /// Whether an expression is an Option/Result/Missing constructor call
    /// that needs an expected type (`None`/`Missing` bare, or
    /// `Some`/`Ok`/`Err`/`Present` calls). User bindings/functions with
    /// those names shadow the builtins.
    fn ctor_use(&self, e: &Expr) -> Option<(String, Vec<Expr>)> {
        match &e.kind {
            ExprKind::Var(n) if n == "None" && !self.shadowed(n) => {
                Some(("None".to_string(), vec![]))
            }
            ExprKind::Var(n) if n == "Missing" && !self.shadowed(n) => {
                Some(("Missing".to_string(), vec![]))
            }
            ExprKind::Call { name, args }
                if name == "None" && args.is_empty() && !self.shadowed(name) =>
            {
                Some(("None".to_string(), vec![]))
            }
            ExprKind::Call { name, args }
                if name == "Missing" && args.is_empty() && !self.shadowed(name) =>
            {
                Some(("Missing".to_string(), vec![]))
            }
            ExprKind::Call { name, args }
                if (name == "Some" || name == "Ok" || name == "Err" || name == "Present")
                    && self.resolve_fun(name).is_none() =>
            {
                Some((name.clone(), args.clone()))
            }
            _ => None,
        }
    }

    /// Check `None` / `Some(v)` / `Ok(v)` / `Err(e)` against an optional
    /// expected `Option<T>` / `Result<T, E>` type. Without enough information
    /// (bare `None`, `Ok`/`Err` without annotation) this is E0201 rather than
    /// a guess. Returns the constructed type.
    fn check_ctor_call(
        &mut self,
        id: crate::ast::NodeId,
        span: Span,
        name: &str,
        args: &[Expr],
        expected: Option<&Ty>,
    ) -> Ty {
        let ret = self.check_ctor_call_inner(span, name, args, expected);
        self.types.insert(id, ret.clone());
        ret
    }

    fn check_ctor_call_inner(
        &mut self,
        span: Span,
        name: &str,
        args: &[Expr],
        expected: Option<&Ty>,
    ) -> Ty {
        // Validate arity first.
        let arity_ok = match name {
            "None" | "Missing" => {
                if !args.is_empty() {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes no arguments", name),
                        None,
                    );
                    false
                } else {
                    true
                }
            }
            "Some" | "Ok" | "Err" | "Present" => {
                if args.len() != 1 {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes exactly 1 argument", name),
                        None,
                    );
                    false
                } else {
                    true
                }
            }
            _ => false,
        };
        if !arity_ok {
            return Ty::Error;
        }
        match name {
            "None" => match expected {
                Some(Ty::Generic { base, args }) if base == "Option" && args.len() == 1 => {
                    Ty::Generic {
                        base: base.clone(),
                        args: args.clone(),
                    }
                }
                Some(other) => {
                    if ty_has_error(other) {
                        return Ty::Error;
                    }
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`None` needs an `Option<T>` context, found {}",
                            other.name()
                        ),
                        None,
                    );
                    Ty::Error
                }
                None => {
                    self.err(
                        "E0201",
                        span,
                        "`None` needs an expected `Option<T>` type; annotate the binding",
                        Some("e.g. `val x: Option<Int32> = None`".into()),
                    );
                    Ty::Error
                }
            },
            "Missing" => match expected {
                Some(Ty::Generic { base, args }) if base == "Missing" && args.len() == 1 => {
                    Ty::Generic {
                        base: base.clone(),
                        args: args.clone(),
                    }
                }
                Some(other) => {
                    if ty_has_error(other) {
                        return Ty::Error;
                    }
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`Missing` needs a `Missing<T>` context, found {}",
                            other.name()
                        ),
                        None,
                    );
                    Ty::Error
                }
                None => {
                    self.err(
                        "E0201",
                        span,
                        "`Missing` needs an expected `Missing<T>` type; annotate the binding",
                        Some("e.g. `val x: Missing<Float64> = Missing`".into()),
                    );
                    Ty::Error
                }
            },
            "Some" | "Present" => {
                let want = if name == "Some" { "Option" } else { "Missing" };
                let at = self.check_expr(&args[0]);
                match expected {
                    Some(Ty::Generic { base, args: exp }) if base == want && exp.len() == 1 => {
                        if exp[0] != at && !ty_has_error(&exp[0]) && !ty_has_error(&at) {
                            self.err(
                                "E0201",
                                args[0].span,
                                format!("expected {}, found {}", exp[0].name(), at.name()),
                                None,
                            );
                        }
                        Ty::Generic {
                            base: base.clone(),
                            args: exp.clone(),
                        }
                    }
                    Some(other) => {
                        if ty_has_error(other) {
                            return Ty::Error;
                        }
                        self.err(
                            "E0201",
                            span,
                            format!(
                                "`{}(..)` needs a `{}<T>` context, found {}",
                                name,
                                want,
                                other.name()
                            ),
                            None,
                        );
                        Ty::Error
                    }
                    None => {
                        // Record the instantiation for the inferred payload.
                        let key = Self::inst_key(want, std::slice::from_ref(&at));
                        self.generic_insts
                            .insert(key, (want.to_string(), vec![at.clone()]));
                        Ty::Generic {
                            base: want.to_string(),
                            args: vec![at],
                        }
                    }
                }
            }
            "Ok" | "Err" => {
                let is_ok = name == "Ok";
                let at = self.check_expr(&args[0]);
                match expected {
                    Some(Ty::Generic { base, args: exp }) if base == "Result" && exp.len() == 2 => {
                        let slot = if is_ok { &exp[0] } else { &exp[1] };
                        if *slot != at && !ty_has_error(slot) && !ty_has_error(&at) {
                            self.err(
                                "E0201",
                                args[0].span,
                                format!("expected {}, found {}", slot.name(), at.name()),
                                None,
                            );
                        }
                        Ty::Generic {
                            base: base.clone(),
                            args: exp.clone(),
                        }
                    }
                    Some(other) => {
                        if ty_has_error(other) {
                            return Ty::Error;
                        }
                        self.err(
                            "E0201",
                            span,
                            format!(
                                "`{}` needs a `Result<T, E>` context, found {}",
                                name,
                                other.name()
                            ),
                            None,
                        );
                        Ty::Error
                    }
                    None => {
                        self.err(
                            "E0201",
                            span,
                            format!(
                                "`{}` needs an expected `Result<T, E>` type; annotate the binding",
                                name
                            ),
                            Some("e.g. `val x: Result<Int32, String> = Ok(1)`".into()),
                        );
                        Ty::Error
                    }
                }
            }
            _ => Ty::Unit,
        }
    }

    /// Check a builtin `Option::`/`Result::` construction. `expected` is
    /// the ambient annotated type, if any.
    fn check_builtin_variant(
        &mut self,
        span: Span,
        ty: &str,
        variant: &str,
        var_span: Span,
        args: &[Expr],
        expected: Option<&Ty>,
    ) -> Ty {
        let shape: Option<(usize, bool)> = match (ty, variant) {
            ("Option", "Some") => Some((1, false)),
            ("Option", "None") => Some((0, false)),
            ("Result", "Ok") => Some((1, false)),
            ("Result", "Err") => Some((1, false)),
            ("Missing", "Present") => Some((1, false)),
            ("Missing", "Missing") => Some((0, false)),
            _ => None,
        };
        let (arity, _) = match shape {
            Some(s) => s,
            None => {
                self.err(
                    "E0401",
                    var_span,
                    format!("unknown variant `{}::{}`", ty, variant),
                    None,
                );
                return Ty::Error;
            }
        };
        if args.len() != arity {
            self.err(
                "E0201",
                span,
                format!("`{}::{}` takes exactly {} argument(s)", ty, variant, arity),
                None,
            );
            return Ty::Error;
        }
        // Expected instantiation, when it names the same builtin.
        let exp_args: Option<Vec<Ty>> = match expected {
            Some(Ty::Generic { base, args }) if base == ty => Some(args.clone()),
            _ => None,
        };
        let mut targs: Vec<Ty> = vec![];
        if arity == 1 {
            let at = self.check_expr(&args[0]);
            // Positional slot this payload fills: Some/Present->0, Ok->0, Err->1.
            let slot = match (ty, variant) {
                ("Option", "Some") => 0,
                ("Missing", "Present") => 0,
                ("Result", "Ok") => 0,
                ("Result", "Err") => 1,
                _ => 0,
            };
            let width = if ty == "Result" { 2 } else { 1 };
            targs = vec![Ty::Unit; width];
            // If the expected type knows this slot, validate against it.
            if let Some(exp) = exp_args {
                if exp.len() == width
                    && exp[slot] != at
                    && !ty_has_error(&exp[slot])
                    && !ty_has_error(&at)
                {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("expected {}, found {}", exp[slot].name(), at.name()),
                        None,
                    );
                }
                targs = exp;
            } else {
                targs[slot] = at;
                // No expected type: the other side (if any) is unknowable.
                if width == 2 {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`{}::{}` needs an expected `Result<T, E>` type; annotate the binding",
                            ty, variant
                        ),
                        Some("e.g. `val x: Result<Int32, String> = Ok(1)`".into()),
                    );
                    return Ty::Error;
                }
                if ty == "Option" && variant == "None" || ty == "Missing" && variant == "Missing" {
                    unreachable!("arity-0 handled below");
                }
            }
        } else {
            // Arity 0 (`None`, `Missing::Missing`): payload unknowable
            // without context.
            match exp_args {
                Some(exp) => targs = exp,
                None => {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`{}::{}` needs an expected type; annotate the binding",
                            ty, variant
                        ),
                        None,
                    );
                    return Ty::Error;
                }
            }
        }
        let key = Self::inst_key(ty, &targs);
        self.generic_insts
            .insert(key, (ty.to_string(), targs.clone()));
        Ty::Generic {
            base: ty.to_string(),
            args: targs,
        }
    }

    /// Check tuple-struct/enum-variant positional construction against
    /// expected field types. Returns the constructed nominal type.
    fn check_tuple_ctor(
        &mut self,
        span: Span,
        what: &str,
        fields: &[(String, Ty)],
        args: &[Expr],
        ret: Ty,
    ) -> Ty {
        if args.len() != fields.len() {
            self.err(
                "E0201",
                span,
                format!(
                    "`{}` takes {} arguments, found {}",
                    what,
                    fields.len(),
                    args.len()
                ),
                None,
            );
        }
        for (a, (_, ft)) in args.iter().zip(fields.iter()) {
            let at = self.check_expr(a);
            if at != *ft && !ty_has_error(&at) && !ty_has_error(ft) {
                self.err(
                    "E0201",
                    a.span,
                    format!("argument is {}, but field expects {}", at.name(), ft.name()),
                    None,
                );
            }
        }
        ret
    }

    // ---------- match checking (Phase C) ----------

    /// Declare every binding in a pattern as poison when the scrutinee
    /// already errored, so arm bodies stay silent (no `unknown variable`
    /// cascades). Dimensions default; only reached on poisoned input.
    fn declare_error_binds(&mut self, pat: &Pat) {
        match pat {
            Pat::Bind(name, _) => {
                if let Some(m) = self.locals.last_mut() {
                    m.insert(
                        name.clone(),
                        Local {
                            ty: Ty::Error,
                            mutable: false,
                            dim: Dim::dimensionless(),
                        },
                    );
                }
            }
            Pat::Variant { sub, .. } => {
                for s in sub {
                    self.declare_error_binds(s);
                }
            }
            Pat::Struct { fields, .. } => {
                for (fname, sub) in fields {
                    match sub {
                        Some(p) => self.declare_error_binds(p),
                        // `x` shorthand binds the field name itself.
                        None => {
                            self.declare_error_binds(&Pat::Bind(fname.clone(), Span::new(0, 0)))
                        }
                    }
                }
            }
            Pat::Or(alts) => {
                for a in alts {
                    self.declare_error_binds(a);
                }
            }
            _ => {}
        }
    }

    /// Check a pattern against the scrutinee type. Declares bindings in the
    /// current (arm) scope. Returns Err(()) on any error (already reported).
    fn check_pat(
        &mut self,
        pat: &Pat,
        scrut_ty: &Ty,
        scrut_dim: &Dim,
        e_span: Span,
    ) -> Result<(), ()> {
        match pat {
            Pat::Wild => Ok(()),
            Pat::LitInt(v) => {
                let want = if *v > i32::MAX as i64 || *v < i32::MIN as i64 {
                    Ty::Int64
                } else {
                    Ty::Int32
                };
                if *scrut_ty != want && !ty_has_error(scrut_ty) {
                    self.err(
                        "E0201",
                        e_span,
                        format!(
                            "pattern `{}` has type {}, but scrutinee is {}",
                            v,
                            want.name(),
                            scrut_ty.name()
                        ),
                        Some("patterns must match the scrutinee type exactly".into()),
                    );
                    return Err(());
                }
                Ok(())
            }
            Pat::LitFloat(_) => {
                if *scrut_ty != Ty::Float64 && !ty_has_error(scrut_ty) {
                    self.err(
                        "E0201",
                        e_span,
                        format!(
                            "float pattern needs a Float64 scrutinee, found {}",
                            scrut_ty.name()
                        ),
                        None,
                    );
                    return Err(());
                }
                Ok(())
            }
            Pat::LitBool(_) => {
                if *scrut_ty != Ty::Bool && !ty_has_error(scrut_ty) {
                    self.err(
                        "E0201",
                        e_span,
                        format!(
                            "boolean pattern needs a Bool scrutinee, found {}",
                            scrut_ty.name()
                        ),
                        None,
                    );
                    return Err(());
                }
                Ok(())
            }
            Pat::LitStr(_) => {
                if *scrut_ty != Ty::Str && !ty_has_error(scrut_ty) {
                    self.err(
                        "E0201",
                        e_span,
                        format!(
                            "string pattern needs a String scrutinee, found {}",
                            scrut_ty.name()
                        ),
                        None,
                    );
                    return Err(());
                }
                Ok(())
            }
            Pat::Bind(name, span) => {
                if self
                    .locals
                    .last()
                    .map(|m| m.contains_key(name))
                    .unwrap_or(false)
                {
                    self.err(
                        "E0401",
                        *span,
                        format!("duplicate binding `{}` in the same pattern", name),
                        None,
                    );
                    return Err(());
                }
                self.locals.last_mut().unwrap().insert(
                    name.clone(),
                    Local {
                        ty: scrut_ty.clone(),
                        mutable: false,
                        dim: scrut_dim.clone(),
                    },
                );
                Ok(())
            }
            Pat::Variant {
                ty,
                variant,
                span,
                sub,
            } => {
                // Poisoned scrutinee: declare bindings as poison and stay
                // silent (bodies still check; their uses stay silent too).
                if ty_has_error(scrut_ty) {
                    self.declare_error_binds(pat);
                    return Ok(());
                }
                // Resolve the enum: qualified must match, bare resolves via scrutinee.
                // `Option`/`Result`/`Missing` (builtin generics) resolve structurally.
                let ename = match ty {
                    Some(t) if t == "Option" || t == "Result" || t == "Missing" => t.clone(),
                    Some(t) => {
                        if let Some(q) = self.resolve_type(t).filter(|q| self.enums.contains_key(q))
                        {
                            q
                        } else {
                            self.err(
                                "E0401",
                                *span,
                                format!("unknown type `{}` in pattern", t),
                                None,
                            );
                            return Err(());
                        }
                    }
                    None => match scrut_ty {
                        Ty::Enum(n) => n.clone(),
                        Ty::Generic { base, .. }
                            if base == "Option" || base == "Result" || base == "Missing" =>
                        {
                            base.clone()
                        }
                        _ => {
                            self.err(
                                "E0201",
                                *span,
                                format!(
                                    "bare variant pattern `{}` needs an enum scrutinee, found {}",
                                    variant,
                                    scrut_ty.name()
                                ),
                                Some("qualify it as `Type::{}`".into()),
                            );
                            return Err(());
                        }
                    },
                };
                let payload: Vec<Ty> = match self.enums.get(&ename).cloned() {
                    Some(d) => match d.variants.iter().find(|(v, _)| v == variant) {
                        Some((_, p)) => p.clone(),
                        None => {
                            self.err(
                                "E0401",
                                *span,
                                format!("unknown variant `{}::{}`", ename, variant),
                                None,
                            );
                            return Err(());
                        }
                    },
                    None => {
                        // Builtin generic payloads (sema guarantees instantiation).
                        match (ename.as_str(), variant.as_str(), scrut_ty) {
                            ("Option", "Some", Ty::Generic { args, .. }) if args.len() == 1 => {
                                vec![args[0].clone()]
                            }
                            ("Option", "None", Ty::Generic { args, .. }) if args.len() == 1 => {
                                vec![]
                            }
                            ("Missing", "Present", Ty::Generic { args, .. }) if args.len() == 1 => {
                                vec![args[0].clone()]
                            }
                            ("Missing", "Missing", Ty::Generic { args, .. }) if args.len() == 1 => {
                                vec![]
                            }
                            ("Result", "Ok", Ty::Generic { args, .. }) if args.len() == 2 => {
                                vec![args[0].clone()]
                            }
                            ("Result", "Err", Ty::Generic { args, .. }) if args.len() == 2 => {
                                vec![args[1].clone()]
                            }
                            _ => {
                                self.err(
                                    "E0401",
                                    *span,
                                    format!("unknown variant `{}::{}`", ename, variant),
                                    None,
                                );
                                return Err(());
                            }
                        }
                    }
                };
                // Qualified patterns must name the scrutinee's own type.
                match scrut_ty {
                    Ty::Enum(sn) if *sn != ename => {
                        self.err(
                            "E0201",
                            *span,
                            format!(
                                "pattern on `{}` cannot match scrutinee of type `{}`",
                                ename, sn
                            ),
                            None,
                        );
                        return Err(());
                    }
                    Ty::Generic { base, .. } if base != &ename => {
                        self.err(
                            "E0201",
                            *span,
                            format!(
                                "pattern on `{}` cannot match scrutinee of type {}",
                                ename,
                                scrut_ty.name()
                            ),
                            None,
                        );
                        return Err(());
                    }
                    _ => {}
                }
                if sub.len() != payload.len() {
                    self.err(
                        "E0201",
                        *span,
                        format!(
                            "variant `{}::{}` takes {} sub-patterns, found {}",
                            ename,
                            variant,
                            payload.len(),
                            sub.len()
                        ),
                        None,
                    );
                    return Err(());
                }
                for (sp, pt) in sub.iter().zip(payload.iter()) {
                    self.check_pat(sp, pt, &Dim::dimensionless(), e_span)?;
                }
                Ok(())
            }
            Pat::Struct { name, fields } => {
                if ty_has_error(scrut_ty) {
                    self.declare_error_binds(pat);
                    return Ok(());
                }
                let qname = self.resolve_type(name);
                let def = match qname.as_ref().and_then(|q| self.structs.get(q)).cloned() {
                    Some(d) => d,
                    None => {
                        self.err(
                            "E0401",
                            e_span,
                            format!("unknown struct `{}` in pattern", name),
                            None,
                        );
                        return Err(());
                    }
                };
                if *scrut_ty != Ty::Struct(qname.clone().unwrap_or_else(|| name.clone())) {
                    self.err(
                        "E0201",
                        e_span,
                        format!(
                            "struct pattern on `{}` cannot match scrutinee of type {}",
                            name,
                            scrut_ty.name()
                        ),
                        None,
                    );
                    return Err(());
                }
                for (fname, sub) in fields {
                    match def.fields.iter().find(|(f, _)| f == fname) {
                        None => {
                            self.err(
                                "E0401",
                                e_span,
                                format!("unknown field `{}.{}` in pattern", name, fname),
                                None,
                            );
                            return Err(());
                        }
                        Some((_, fty)) => {
                            match sub {
                                None => {
                                    // `x` shorthand binds the field.
                                    if self
                                        .locals
                                        .last()
                                        .map(|m| m.contains_key(fname))
                                        .unwrap_or(false)
                                    {
                                        self.err(
                                            "E0401",
                                            e_span,
                                            format!(
                                                "duplicate binding `{}` in the same pattern",
                                                fname
                                            ),
                                            None,
                                        );
                                        return Err(());
                                    }
                                    self.locals.last_mut().unwrap().insert(
                                        fname.clone(),
                                        Local {
                                            ty: fty.clone(),
                                            mutable: false,
                                            dim: Dim::dimensionless(),
                                        },
                                    );
                                }
                                Some(p) => {
                                    self.check_pat(p, fty, &Dim::dimensionless(), e_span)?;
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
            Pat::Or(alts) => {
                // Every alternative must bind exactly the same names/types.
                let mut first: Option<Vec<(String, Ty, Dim)>> = None;
                for a in alts {
                    let before: Vec<String> = self
                        .locals
                        .last()
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                    self.check_pat(a, scrut_ty, scrut_dim, e_span)?;
                    let mut got: Vec<(String, Ty, Dim)> = self
                        .locals
                        .last()
                        .map(|m| {
                            m.iter()
                                .filter(|(k, _)| !before.contains(k))
                                .map(|(k, v)| (k.clone(), v.ty.clone(), v.dim.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    got.sort_by(|a, b| a.0.cmp(&b.0));
                    match &first {
                        None => first = Some(got.clone()),
                        Some(f) => {
                            if *f != got {
                                self.err(
                                    "E0201",
                                    e_span,
                                    "`|` alternatives must bind the same names with the same types",
                                    None,
                                );
                                return Err(());
                            }
                        }
                    }
                    // Remove this alternative's bindings; the merged set is
                    // declared after the loop so later arms start clean.
                    if let Some(scope) = self.locals.last_mut() {
                        for (k, _, _) in &got {
                            scope.remove(k);
                        }
                    }
                }
                if let Some(bindings) = first {
                    if let Some(scope) = self.locals.last_mut() {
                        for (k, t, d) in bindings {
                            scope.insert(
                                k,
                                Local {
                                    ty: t,
                                    mutable: false,
                                    dim: d,
                                },
                            );
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// Shape-only coverage (literals false, binds/wilds true, recurse).
    fn pat_shape_covers(&self, pat: &Pat) -> bool {
        match pat {
            Pat::Wild => true,
            Pat::Bind(_, _) => true,
            Pat::Variant { sub, .. } => sub.iter().all(|p| self.pat_shape_covers(p)),
            Pat::Struct { fields, .. } => fields.iter().all(|(_, sub)| {
                sub.as_ref()
                    .map(|p| self.pat_shape_covers(p))
                    .unwrap_or(true)
            }),
            Pat::Or(alts) => alts.iter().all(|a| self.pat_shape_covers(a)),
            _ => false,
        }
    }

    /// Missing coverage descriptions for E0204, or empty when exhaustive.
    fn missing_coverage(&self, scrut_ty: &Ty, arms: &[(Pat, bool)]) -> Vec<String> {
        // Any unguarded wildcard/binding covers everything.
        for (p, guarded) in arms {
            if !guarded {
                match p {
                    Pat::Wild => return vec![],
                    Pat::Bind(_, _) => return vec![],
                    Pat::Or(alts)
                        if alts
                            .iter()
                            .any(|a| matches!(a, Pat::Wild | Pat::Bind(_, _))) =>
                    {
                        return vec![]
                    }
                    _ => {}
                }
            }
        }
        match scrut_ty {
            Ty::Bool => {
                let mut missing = vec![];
                let has = |b: bool| {
                    arms.iter().any(|(p, g)| {
                        !g && matches!(p, Pat::LitBool(v) if *v == b)
                            || !g && matches!(p, Pat::Or(alts) if alts.iter().any(|a| matches!(a, Pat::LitBool(v) if *v == b)))
                    })
                };
                if !has(true) {
                    missing.push("true".to_string());
                }
                if !has(false) {
                    missing.push("false".to_string());
                }
                missing
            }
            Ty::Generic { base, .. }
                if base == "Option" || base == "Result" || base == "Missing" =>
            {
                let variants: Vec<(&str, usize)> = if base == "Option" {
                    vec![("Some", 1), ("None", 0)]
                } else if base == "Missing" {
                    vec![("Present", 1), ("Missing", 0)]
                } else {
                    vec![("Ok", 1), ("Err", 1)]
                };
                variants
                    .iter()
                    .filter(|(v, arity)| {
                        !arms
                            .iter()
                            .any(|(p, g)| !g && self.pat_covers_generic(p, base, v, *arity))
                    })
                    .map(|(v, _)| format!("{}(_)", v))
                    .collect()
            }
            Ty::Enum(n) => match self.enums.get(n) {
                Some(def) => def
                    .variants
                    .iter()
                    .filter(|(v, pts)| {
                        !arms
                            .iter()
                            .any(|(p, g)| !g && self.pat_covers_variant(p, n, v, pts.len()))
                    })
                    .map(|(v, _)| format!("{}::{}(_)", n, v))
                    .collect(),
                None => vec!["`_`".to_string()],
            },
            _ => vec!["`_`".to_string()],
        }
    }

    /// Does unguarded pattern `p` cover a builtin-generic variant
    /// (`Some`/`None`, `Ok`/`Err`)? Qualified must name the same base.
    fn pat_covers_generic(&self, p: &Pat, base: &str, vname: &str, arity: usize) -> bool {
        match p {
            Pat::Variant {
                ty, variant, sub, ..
            } => {
                let matches_ty = match ty {
                    Some(t) => t == base,
                    None => true,
                };
                matches_ty
                    && variant == vname
                    && sub.len() == arity
                    && sub.iter().all(|s| self.pat_shape_covers(s))
            }
            Pat::Or(alts) => alts
                .iter()
                .any(|a| self.pat_covers_generic(a, base, vname, arity)),
            _ => false,
        }
    }

    /// Does unguarded pattern `p` cover enum variant `v` (arity checked)?
    fn pat_covers_variant(&self, p: &Pat, ename: &str, vname: &str, arity: usize) -> bool {
        match p {
            Pat::Variant {
                ty, variant, sub, ..
            } => {
                let matches_ty = match ty {
                    // Qualified arm types resolve through the current module +
                    // imports before comparing (checked for validity already).
                    Some(t) => self.resolve_type(t).as_deref() == Some(ename),
                    None => true, // bare resolves to the scrutinee enum (checked)
                };
                matches_ty
                    && variant == vname
                    && sub.len() == arity
                    && sub.iter().all(|s| self.pat_shape_covers(s))
            }
            Pat::Or(alts) => alts
                .iter()
                .any(|a| self.pat_covers_variant(a, ename, vname, arity)),
            _ => false,
        }
    }

    /// Check a match (statement or value). Returns the match type for value
    /// position; statement position requires Unit arms.
    fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        span: Span,
        ret_ty: &Ty,
        value: bool,
    ) -> (Ty, Dim) {
        let st = self.check_expr(scrutinee);
        // Hopeless category error (e.g. `5 ?? 6` desugar): variant patterns
        // against a concrete non-enum scrutinee can never match. Diagnose
        // once and poison instead of cascading per-arm errors.
        let hopeless = match &st {
            Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str | Ty::Unit | Ty::Struct(_) => {
                true
            }
            Ty::Generic { base, .. } => base == "Vec" || base == "Mat" || base == "Factor",
            _ => false,
        };
        if hopeless && arms.iter().any(|a| matches!(&a.pat, Pat::Variant { .. })) {
            self.err(
                "E0201",
                span,
                format!("cannot match on {} with variant patterns", st.name()),
                Some("match on an enum, `Option`, `Result`, or `Missing` value".into()),
            );
            return (Ty::Error, Dim::dimensionless());
        }
        let mut first_ty: Option<Ty> = None;
        let mut first_dim: Option<Dim> = None;
        let mut result = Ty::Unit;
        for arm in arms {
            self.locals.push(HashMap::new());
            let sdim = self.dim_of(scrutinee.id);
            let ok = self.check_pat(&arm.pat, &st, &sdim, span).is_ok();
            if let Some(g) = &arm.guard {
                let gt = self.check_expr(g);
                if gt != Ty::Bool && !ty_has_error(&gt) {
                    self.err(
                        "E0201",
                        g.span,
                        format!("match guard must be `Bool`, found {}", gt.name()),
                        None,
                    );
                }
            }
            if value {
                let _ = self.check_block_expr(&arm.body);
            } else {
                let (bt, bflow) = self.check_block(&arm.body, ret_ty);
                if matches!(bflow, Flow::Normal) && bt != Ty::Unit && !ty_has_error(&bt) {
                    self.err(
                        "E0201",
                        arm.body.span,
                        "statement `match` arms must evaluate to `()`",
                        Some("add `;` or restructure with a value `match`".into()),
                    );
                }
            }
            // Arm value type (value position): block tail or Unit.
            // First arm's dimension wins (codegen converts the rest).
            if value && ok {
                let at = self.types.get(&arm.body.id).cloned().unwrap_or(Ty::Unit);
                match &first_ty {
                    None => {
                        first_ty = Some(at.clone());
                        result = at;
                        first_dim = Some(self.dim_of(arm.body.id));
                    }
                    Some(f) => {
                        if *f != at && !ty_has_error(f) && !ty_has_error(&at) {
                            self.err(
                                "E0201",
                                arm.span,
                                format!("`match` arms disagree: {} vs {}", f.name(), at.name()),
                                None,
                            );
                        }
                        if let Some(fd) = &first_dim {
                            // Literal tails pin silently to the first arm.
                            let pinned = match &arm.body.tail {
                                Some(t) if Self::is_dimless_literal(t) => {
                                    self.set_dim(t.id, fd.clone());
                                    true
                                }
                                _ => false,
                            };
                            if !pinned {
                                let ad = self.dim_of(arm.body.id);
                                if !fd.same_dim(&ad) {
                                    self.err(
                                        "E0201",
                                        arm.span,
                                        format!(
                                            "`match` arms disagree in dimension: {} vs {}",
                                            fd.display(),
                                            ad.display()
                                        ),
                                        Some("dimensions must agree; convert explicitly".into()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            self.locals.pop();
        }
        if value {
            let cov: Vec<(Pat, bool)> = arms
                .iter()
                .map(|a| (a.pat.clone(), a.guard.is_some()))
                .collect();
            let missing = self.missing_coverage(&st, &cov);
            if !missing.is_empty() && !ty_has_error(&st) {
                self.err(
                    "E0204",
                    span,
                    format!(
                        "non-exhaustive `match` on {} (missing: {})",
                        st.name(),
                        missing.join(", ")
                    ),
                    Some("add a `_` arm".into()),
                );
            }
        }
        if value {
            // Borrow-position matches clone arm values into the result
            // slot: record the type so its clone helper is emitted.
            self.record_ty(&result);
            (result, first_dim.unwrap_or_else(Dim::dimensionless))
        } else {
            (Ty::Unit, Dim::dimensionless())
        }
    }

    /// `Vec` method resolution (0.1.0 subset; closures/slices deferred).
    /// Mutating methods take the vec by exclusive use and return the updated
    /// value (`v = v.push(x)`); reads never alias the buffer (get clones).
    fn check_vec_method(
        &mut self,
        span: Span,
        elem: &Ty,
        method: &str,
        method_span: Span,
        args: &[Expr],
    ) -> Ty {
        // Numeric kernel element types (Float64 + ints; mixed rejected).
        let num = matches!(elem, Ty::Float64 | Ty::Int32 | Ty::Int64);
        let shape_err = Ty::Struct("ShapeError".to_string());
        let vec_of = |t: Ty| Ty::Generic {
            base: "Vec".to_string(),
            args: vec![t],
        };
        let opt_of = |t: Ty| Ty::Generic {
            base: "Option".to_string(),
            args: vec![t],
        };
        let res_of = |t: Ty| Ty::Generic {
            base: "Result".to_string(),
            args: vec![t, shape_err.clone()],
        };
        match method {
            "len" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`len` takes no arguments", None);
                }
                Ty::Int64
            }
            "push" => {
                // In-place mutation through the vec slot (pointer model);
                // returns Unit, so `v.push(x);` is a complete statement.
                if args.len() != 1 {
                    self.err("E0201", span, "`push` takes exactly 1 argument", None);
                    return Ty::Error;
                }
                // Constructors (`None`, `Missing`) check against the
                // element type, like annotated call arguments.
                let at = match self.ctor_use(&args[0]) {
                    Some((cn, cargs)) => {
                        self.check_ctor_call(args[0].id, args[0].span, &cn, &cargs, Some(elem))
                    }
                    None => self.check_expr(&args[0]),
                };
                if at != *elem && !ty_has_error(&at) && !ty_has_error(elem) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`push` expects {}, found {}", elem.name(), at.name()),
                        None,
                    );
                }
                Ty::Unit
            }
            "pop" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`pop` takes no arguments", None);
                }
                let key = Self::inst_key("Option", std::slice::from_ref(elem));
                self.generic_insts
                    .insert(key, ("Option".to_string(), vec![elem.clone()]));
                opt_of(elem.clone())
            }
            "get" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`get` takes exactly 1 argument", None);
                    return opt_of(elem.clone());
                }
                let at = self.check_expr(&args[0]);
                if !matches!(at, Ty::Int32 | Ty::Int64) && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`get` index must be Int32/Int64, found {}", at.name()),
                        None,
                    );
                }
                let key = Self::inst_key("Option", std::slice::from_ref(elem));
                self.generic_insts
                    .insert(key, ("Option".to_string(), vec![elem.clone()]));
                opt_of(elem.clone())
            }
            "clone" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`clone` takes no arguments", None);
                }
                self.record_vec_inst(elem);
                vec_of(elem.clone())
            }
            "cast" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`cast` takes no arguments", None);
                }
                match self.expected.clone() {
                    Some(Ty::Generic { base, args }) if base == "Vec" && args.len() == 1 => {
                        let key = Self::inst_key("Vec", std::slice::from_ref(&args[0]));
                        self.generic_insts
                            .insert(key, ("Vec".to_string(), vec![args[0].clone()]));
                        vec_of(args[0].clone())
                    }
                    _ => {
                        self.err(
                            "E0201",
                            span,
                            "`cast` target comes from the annotation; annotate the binding",
                            Some("e.g. `val f: Vec<Float64> = ints.cast()`".into()),
                        );
                        Ty::Error
                    }
                }
            }
            "add_scalar" => {
                if !num {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`add_scalar` needs a numeric `Vec`, found Vec<{}>",
                            elem.name()
                        ),
                        None,
                    );
                    return vec_of(elem.clone());
                }
                if args.len() != 1 {
                    self.err("E0201", span, "`add_scalar` takes exactly 1 argument", None);
                    return vec_of(elem.clone());
                }
                let at = self.check_expr(&args[0]);
                if at != *elem && !ty_has_error(&at) && !ty_has_error(elem) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`add_scalar` expects {}, found {}", elem.name(), at.name()),
                        None,
                    );
                }
                self.record_vec_inst(elem);
                vec_of(elem.clone())
            }
            "add" | "mul" => {
                if !num {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`{}` needs a numeric `Vec`, found Vec<{}>",
                            method,
                            elem.name()
                        ),
                        None,
                    );
                    return vec_of(elem.clone());
                }
                if args.len() != 1 {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes exactly 1 argument", method),
                        None,
                    );
                    return res_of(vec_of(elem.clone()));
                }
                let at = self.check_expr(&args[0]);
                let want = vec_of(elem.clone());
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`{}` expects {}, found {}", method, want.name(), at.name()),
                        None,
                    );
                }
                self.record_vec_inst(elem);
                res_of(vec_of(elem.clone()))
            }
            "dot" => {
                if *elem != Ty::Float64 && !ty_has_error(elem) {
                    self.err(
                        "E0201",
                        span,
                        format!("`dot` needs `Vec<Float64>`, found Vec<{}>", elem.name()),
                        None,
                    );
                    return Ty::Float64;
                }
                if args.len() != 1 {
                    self.err("E0201", span, "`dot` takes exactly 1 argument", None);
                    return res_of(Ty::Float64);
                }
                let at = self.check_expr(&args[0]);
                let want = vec_of(Ty::Float64);
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`dot` expects {}, found {}", want.name(), at.name()),
                        None,
                    );
                }
                res_of(Ty::Float64)
            }
            "shuffle" => {
                // In-place Fisher-Yates over any element type; the Rng
                // advances through the receiver pointer model.
                if args.len() != 1 {
                    self.err("E0201", span, "`shuffle` takes exactly 1 argument", None);
                    return Ty::Error;
                }
                let at = self.check_expr(&args[0]);
                let want = Ty::Generic {
                    base: "Rng".to_string(),
                    args: vec![],
                };
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`shuffle` expects Rng, found {}", at.name()),
                        None,
                    );
                }
                Ty::Unit
            }
            "sample" => {
                // Random subset without replacement; `n` clamps to length.
                if args.len() != 2 {
                    self.err("E0201", span, "`sample` takes exactly 2 arguments", None);
                    return vec_of(elem.clone());
                }
                let nt = self.check_expr(&args[0]);
                if nt != Ty::Int64 && nt != Ty::Int32 && !ty_has_error(&nt) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`sample` expects an integer count, found {}", nt.name()),
                        None,
                    );
                }
                let at = self.check_expr(&args[1]);
                let want = Ty::Generic {
                    base: "Rng".to_string(),
                    args: vec![],
                };
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!("`sample` expects Rng, found {}", at.name()),
                        None,
                    );
                }
                let t = vec_of(elem.clone());
                self.record_vec_inst(elem);
                t
            }
            "sort" => {
                // In-place stable sort (bottom-up mergesort, NaN sorts
                // last); numeric, Bool, and String elements only.
                if !args.is_empty() {
                    self.err("E0201", span, "`sort` takes no arguments", None);
                }
                match elem {
                    Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str => Ty::Unit,
                    other => {
                        if !ty_has_error(other) {
                            self.err(
                                "E0201",
                                span,
                                format!("`sort` needs Int32/Int64/Float64/Bool/String elements, found Vec<{}>", other.name()),
                                None,
                            );
                        }
                        Ty::Unit
                    }
                }
            }
            "order" => {
                // Permutation indices such that `v[order[i]]` is sorted;
                // ties keep original order (stable). Same element gate.
                if !args.is_empty() {
                    self.err("E0201", span, "`order` takes no arguments", None);
                }
                match elem {
                    Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str => {
                        self.record_vec_inst(&Ty::Int64);
                        vec_of(Ty::Int64)
                    }
                    other => {
                        if !ty_has_error(other) {
                            self.err(
                                "E0201",
                                span,
                                format!("`order` needs Int32/Int64/Float64/Bool/String elements, found Vec<{}>", other.name()),
                                None,
                            );
                        }
                        vec_of(Ty::Int64)
                    }
                }
            }
            _ => {
                self.err(
                    "E0401",
                    method_span,
                    format!("unknown method `{}` (available on Vec: len, push, pop, get, clone, cast, add_scalar, add, mul, dot, shuffle, sample, sort, order)", method),
                    Some("map/filter/mask take closures or slices (deferred)".into()),
                );
                Ty::Error
            }
        }
    }

    /// `Mat` methods (M1): dense Float64 matrices. Receiver borrows;
    /// fallible kernels report `Result<_, ShapeError>` like the numeric
    /// `Vec` kernels. `det` yields NaN on singular input (total, like
    /// float division); `lu_solve`/`inverse` reject it with `Err`.
    fn check_mat_method(
        &mut self,
        span: Span,
        method: &str,
        method_span: Span,
        args: &[Expr],
    ) -> Ty {
        let mat_ty = || Ty::Generic {
            base: "Mat".to_string(),
            args: vec![],
        };
        let shape_err = Ty::Struct("ShapeError".to_string());
        let res_mat = || Ty::Generic {
            base: "Result".to_string(),
            args: vec![mat_ty(), shape_err.clone()],
        };
        match method {
            "rows" | "cols" => {
                if !args.is_empty() {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes no arguments", method),
                        None,
                    );
                }
                Ty::Int64
            }
            "transpose" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`transpose` takes no arguments", None);
                }
                self.record_ty(&mat_ty());
                mat_ty()
            }
            "matmul" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`matmul` takes exactly 1 argument", None);
                    return res_mat();
                }
                let at = self.check_expr(&args[0]);
                if at != mat_ty() && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`matmul` expects Mat, found {}", at.name()),
                        None,
                    );
                }
                res_mat()
            }
            "lu_solve" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`lu_solve` takes exactly 1 argument", None);
                    return Ty::Generic {
                        base: "Result".to_string(),
                        args: vec![
                            Ty::Generic {
                                base: "Vec".to_string(),
                                args: vec![Ty::Float64],
                            },
                            shape_err.clone(),
                        ],
                    };
                }
                let at = self.check_expr(&args[0]);
                let want = Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Float64],
                };
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`lu_solve` expects Vec<Float64>, found {}", at.name()),
                        None,
                    );
                }
                self.record_vec_inst(&Ty::Float64);
                Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![want, shape_err.clone()],
                }
            }
            "mat_vec" => {
                // S1: matrix-vector product (IRLS needs `X'Wz` without
                // forming diagonals; `m.cols == v.len` else `Err`).
                if args.len() != 1 {
                    self.err("E0201", span, "`mat_vec` takes exactly 1 argument", None);
                    return Ty::Generic {
                        base: "Result".to_string(),
                        args: vec![
                            Ty::Generic {
                                base: "Vec".to_string(),
                                args: vec![Ty::Float64],
                            },
                            shape_err.clone(),
                        ],
                    };
                }
                let at = self.check_expr(&args[0]);
                let want = Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Float64],
                };
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`mat_vec` expects Vec<Float64>, found {}", at.name()),
                        None,
                    );
                }
                self.record_vec_inst(&Ty::Float64);
                Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![want, shape_err.clone()],
                }
            }
            "mat_scale_rows" => {
                // S1: scale each row `i` by `w[i]` (weighted design rows;
                // `m.rows == w.len` else `Err`).
                if args.len() != 1 {
                    self.err(
                        "E0201",
                        span,
                        "`mat_scale_rows` takes exactly 1 argument",
                        None,
                    );
                    return res_mat();
                }
                let at = self.check_expr(&args[0]);
                let want = Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Float64],
                };
                if at != want && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`mat_scale_rows` expects Vec<Float64>, found {}", at.name()),
                        None,
                    );
                }
                self.record_vec_inst(&Ty::Float64);
                res_mat()
            }
            "mat_diag" => {
                // S1: main diagonal as a vector (always total: length is
                // `min(rows, cols)`, so covariance diagonals read cleanly).
                if !args.is_empty() {
                    self.err("E0201", span, "`mat_diag` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Float64);
                Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Float64],
                }
            }
            "det" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`det` takes no arguments", None);
                }
                Ty::Float64
            }
            "inverse" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`inverse` takes no arguments", None);
                }
                res_mat()
            }
            "cholesky" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`cholesky` takes no arguments", None);
                }
                res_mat()
            }
            // Symmetric Jacobi eigen decomposition (values descending, R
            // convention) and one-sided Jacobi SVD (needs rows >= cols).
            // Each is self-contained (recomputed per accessor, documented).
            "eigen_values" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`eigen_values` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Float64);
                Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![
                        Ty::Generic {
                            base: "Vec".to_string(),
                            args: vec![Ty::Float64],
                        },
                        shape_err.clone(),
                    ],
                }
            }
            "eigen_vectors" | "svd_u" | "svd_vt" => {
                if !args.is_empty() {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes no arguments", method),
                        None,
                    );
                }
                // SVD internals stage through Vec<Float64> buffers.
                self.record_vec_inst(&Ty::Float64);
                res_mat()
            }
            "svd_s" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`svd_s` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Float64);
                Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![
                        Ty::Generic {
                            base: "Vec".to_string(),
                            args: vec![Ty::Float64],
                        },
                        shape_err.clone(),
                    ],
                }
            }
            "clone" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`clone` takes no arguments", None);
                }
                self.record_ty(&mat_ty());
                mat_ty()
            }
            _ => {
                self.err(
                    "E0401",
                    method_span,
                    format!("unknown method `{}` (available on Mat: rows, cols, transpose, matmul, mat_vec, mat_scale_rows, mat_diag, eigen_values, eigen_vectors, svd_u, svd_s, svd_vt, lu_solve, det, inverse, cholesky, clone)", method),
                    None,
                );
                Ty::Error
            }
        }
    }

    /// Resolve a Stage-1 function operand to (param types, return type).
    /// Only bare `fun` items qualify (never `main`, methods, or builtins).
    fn fun_value_sig(&mut self, arg: &Expr, what: &str) -> Option<(Vec<Ty>, Ty)> {
        let fname = match &arg.kind {
            ExprKind::Var(n) => n.clone(),
            _ => {
                self.err(
                    "E0401",
                    arg.span,
                    format!(
                        "`{}` needs a named function (e.g. `{}(v, double_it)`); closures and call expressions are deferred",
                        what, what
                    ),
                    None,
                );
                return None;
            }
        };
        if fname == "main" {
            self.err(
                "E0401",
                arg.span,
                format!("`{}` cannot take `main` as a function", what),
                None,
            );
            return None;
        }
        let id = match self.resolve_fun(&fname) {
            Some(id) => id,
            None => {
                self.err(
                    "E0401",
                    arg.span,
                    format!("`{}`: unknown function `{}`", what, fname),
                    Some("pass a `fun` item defined with `fun`".into()),
                );
                return None;
            }
        };
        self.sigs.get(&id).cloned()
    }

    /// Record one functional C kernel instantiation (deduped).
    fn record_fun_kernel(&mut self, kind: &str, tys: Vec<Ty>) {
        let entry = (kind.to_string(), tys);
        if !self.fun_kernels.contains(&entry) {
            self.fun_kernels.push(entry);
        }
    }

    /// C1 Stage-1 functionals over named functions. The function argument
    /// must be a bare `fun` item (no closures, no methods, no builtins);
    /// its signature is checked structurally against the element types.
    /// Records the C kernel instantiation for codegen.
    fn check_functional(&mut self, span: Span, name: &str, args: &[Expr]) -> Ty {
        let scalar = |t: &Ty| matches!(t, Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str);
        let vec_of = |t: Ty| Ty::Generic {
            base: "Vec".to_string(),
            args: vec![t],
        };
        match name {
            "map" => {
                if args.len() != 2 {
                    self.err(
                        "E0201",
                        span,
                        format!("`map` takes exactly 2 arguments, found {}", args.len()),
                        None,
                    );
                    return Ty::Error;
                }
                let vt = self.check_expr(&args[0]);
                if ty_has_error(&vt) {
                    return Ty::Error;
                }
                let t = match vt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        self.err(
                            "E0201",
                            args[0].span,
                            format!("`map` expects Vec<T> first, found {}", vt.name()),
                            None,
                        );
                        return Ty::Error;
                    }
                };
                if !scalar(&t) && !ty_has_error(&t) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!(
                            "`map` supports Int32/Int64/Float64/Bool/String elements, found {}",
                            t.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let Some((ps, r)) = self.fun_value_sig(&args[1], "map") else {
                    return Ty::Error;
                };
                if ps.iter().any(ty_has_error) || ty_has_error(&r) {
                    return Ty::Error;
                }
                if (ps.len() != 1 || ps[0] != t) && !ty_has_error(&t) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!(
                            "`map` function must take exactly one {} (found {})",
                            t.name(),
                            if ps.is_empty() {
                                "no arguments".to_string()
                            } else {
                                ps.iter().map(|p| p.name()).collect::<Vec<_>>().join(", ")
                            }
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                if !scalar(&r) && !ty_has_error(&r) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!(
                            "`map` function must return a scalar/String (found {})",
                            r.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                self.record_vec_inst(&t);
                self.record_vec_inst(&r);
                let key = vec![t.clone(), r.clone()];
                self.record_fun_kernel("map", key);
                vec_of(r)
            }
            "filter" => {
                if args.len() != 2 {
                    self.err(
                        "E0201",
                        span,
                        format!("`filter` takes exactly 2 arguments, found {}", args.len()),
                        None,
                    );
                    return Ty::Error;
                }
                let vt = self.check_expr(&args[0]);
                if ty_has_error(&vt) {
                    return Ty::Error;
                }
                let t = match vt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        self.err(
                            "E0201",
                            args[0].span,
                            format!("`filter` expects Vec<T> first, found {}", vt.name()),
                            None,
                        );
                        return Ty::Error;
                    }
                };
                if !scalar(&t) && !ty_has_error(&t) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!(
                            "`filter` supports Int32/Int64/Float64/Bool/String elements, found {}",
                            t.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let Some((ps, r)) = self.fun_value_sig(&args[1], "filter") else {
                    return Ty::Error;
                };
                if ps.iter().any(ty_has_error) || ty_has_error(&r) {
                    return Ty::Error;
                }
                if (ps.len() != 1 || ps[0] != t || r != Ty::Bool) && !ty_has_error(&t) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!("`filter` predicate must be `({}) -> Bool`", t.name()),
                        None,
                    );
                    return Ty::Error;
                }
                self.record_vec_inst(&t);
                let key = vec![t.clone()];
                self.record_fun_kernel("filter", key);
                vec_of(t)
            }
            "fold" => {
                if args.len() != 3 {
                    self.err(
                        "E0201",
                        span,
                        format!("`fold` takes exactly 3 arguments, found {}", args.len()),
                        None,
                    );
                    return Ty::Error;
                }
                let vt = self.check_expr(&args[0]);
                if ty_has_error(&vt) {
                    return Ty::Error;
                }
                let t = match vt {
                    Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => {
                        args[0].clone()
                    }
                    _ => {
                        self.err(
                            "E0201",
                            args[0].span,
                            format!("`fold` expects Vec<T> first, found {}", vt.name()),
                            None,
                        );
                        return Ty::Error;
                    }
                };
                if !scalar(&t) && !ty_has_error(&t) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!(
                            "`fold` supports Int32/Int64/Float64/Bool/String elements, found {}",
                            t.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let a = self.check_expr(&args[1]);
                if ty_has_error(&a) {
                    return Ty::Error;
                }
                if !scalar(&a) && !ty_has_error(&a) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!(
                            "`fold` accumulator must be a scalar/String (found {})",
                            a.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let Some((ps, r)) = self.fun_value_sig(&args[2], "fold") else {
                    return Ty::Error;
                };
                if ps.iter().any(ty_has_error) || ty_has_error(&r) {
                    return Ty::Error;
                }
                if (ps.len() != 2 || ps[0] != a || ps[1] != t || r != a)
                    && !ty_has_error(&t)
                    && !ty_has_error(&a)
                {
                    self.err(
                        "E0201",
                        args[2].span,
                        format!(
                            "`fold` function must be `({}, {}) -> {}`",
                            a.name(),
                            t.name(),
                            a.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                self.record_vec_inst(&t);
                self.record_vec_inst(&a);
                let key = vec![a.clone(), t.clone()];
                self.record_fun_kernel("fold", key);
                a
            }
            "split_apply" => {
                // Honest tapply core: per-level Float64 groups through a
                // named `(Vec<Float64>) -> Float64` combine (level order).
                if args.len() != 3 {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`split_apply` takes exactly 3 arguments, found {}",
                            args.len()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let ft = self.check_expr(&args[0]);
                if ty_has_error(&ft) {
                    return Ty::Error;
                }
                if !matches!(&ft, Ty::Generic { base, args } if base == "Factor" && args.is_empty())
                {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`split_apply` expects Factor first, found {}", ft.name()),
                        None,
                    );
                    return Ty::Error;
                }
                let vt = self.check_expr(&args[1]);
                let want = vec_of(Ty::Float64);
                if vt != want && !ty_has_error(&vt) {
                    self.err(
                        "E0201",
                        args[1].span,
                        format!(
                            "`split_apply` expects Vec<Float64> second, found {}",
                            vt.name()
                        ),
                        None,
                    );
                    return Ty::Error;
                }
                let Some((ps, r)) = self.fun_value_sig(&args[2], "split_apply") else {
                    return Ty::Error;
                };
                if ps.iter().any(ty_has_error) || ty_has_error(&r) {
                    return Ty::Error;
                }
                if ps.len() != 1 || ps[0] != want || r != Ty::Float64 {
                    self.err(
                        "E0201",
                        args[2].span,
                        "`split_apply` combine must be `(Vec<Float64>) -> Float64`".to_string(),
                        None,
                    );
                    return Ty::Error;
                }
                self.record_vec_inst(&Ty::Float64);
                self.record_ty(&ft);
                self.record_fun_kernel("split_apply", vec![]);
                want
            }
            _ => {
                self.err(
                    "E0401",
                    span,
                    format!("unknown functional `{}`", name),
                    None,
                );
                Ty::Error
            }
        }
    }

    /// `Factor` methods (F1): level/code accessors plus the level-algebra
    /// verbs. Code 0 is missing (`""` on construction, `"NA"` in
    /// `as_strings`); all verbs are total and preserve code-0 entries.
    fn check_factor_method(
        &mut self,
        span: Span,
        method: &str,
        method_span: Span,
        args: &[Expr],
    ) -> Ty {
        let fac_ty = || Ty::Generic {
            base: "Factor".to_string(),
            args: vec![],
        };
        let str_vec = || Ty::Generic {
            base: "Vec".to_string(),
            args: vec![Ty::Str],
        };
        let int_vec = || Ty::Generic {
            base: "Vec".to_string(),
            args: vec![Ty::Int32],
        };
        match method {
            "levels" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`levels` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Str);
                str_vec()
            }
            "codes" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`codes` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Int32);
                int_vec()
            }
            "as_strings" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`as_strings` takes no arguments", None);
                }
                self.record_vec_inst(&Ty::Str);
                str_vec()
            }
            "relevel" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`relevel` takes exactly 1 argument", None);
                    return fac_ty();
                }
                let at = self.check_expr(&args[0]);
                if at != Ty::Str && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`relevel` expects String, found {}", at.name()),
                        None,
                    );
                }
                fac_ty()
            }
            "reorder_by_count" | "droplevels" | "clone" => {
                if !args.is_empty() {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes no arguments", method),
                        None,
                    );
                }
                fac_ty()
            }
            _ => {
                self.err(
                    "E0401",
                    method_span,
                    format!("unknown method `{}` (available on Factor: levels, codes, as_strings, relevel, reorder_by_count, droplevels, clone)", method),
                    None,
                );
                Ty::Error
            }
        }
    }

    /// `String` methods (J1): splitting, trimming, and total parsing.
    /// Parsing yields `Option` (never traps): `None` on any malformed input.
    fn check_str_method(
        &mut self,
        span: Span,
        method: &str,
        method_span: Span,
        args: &[Expr],
    ) -> Ty {
        let opt_of = |t: Ty| Ty::Generic {
            base: "Option".to_string(),
            args: vec![t],
        };
        match method {
            "split" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`split` takes exactly 1 argument", None);
                    return Ty::Generic {
                        base: "Vec".to_string(),
                        args: vec![Ty::Str],
                    };
                }
                let at = self.check_expr(&args[0]);
                if at != Ty::Str && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`split` expects a String delimiter, found {}", at.name()),
                        None,
                    );
                }
                self.record_vec_inst(&Ty::Str);
                Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Str],
                }
            }
            "trim" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`trim` takes no arguments", None);
                }
                Ty::Str
            }
            "clone" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`clone` takes no arguments", None);
                }
                Ty::Str
            }
            "len" => {
                // F2: byte length (no compiler change beyond this accessor;
                // all string verbs build on len/byte_at/slice in stdlib).
                if !args.is_empty() {
                    self.err("E0201", span, "`len` takes no arguments", None);
                }
                Ty::Int64
            }
            "byte_at" => {
                // F2: byte value at a 0-based index (traps out of bounds,
                // exactly like `Vec` indexing).
                if args.len() != 1 {
                    self.err("E0201", span, "`byte_at` takes exactly 1 argument", None);
                    return Ty::Int64;
                }
                let at = self.check_expr(&args[0]);
                if at != Ty::Int64 && at != Ty::Int32 && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`byte_at` expects an Int index, found {}", at.name()),
                        None,
                    );
                }
                Ty::Int64
            }
            "slice" => {
                // F2: `slice(start, end)` byte substring, 0-based half-open
                // (traps on out-of-range bounds, like `byte_at`).
                if args.len() != 2 {
                    self.err("E0201", span, "`slice` takes exactly 2 arguments", None);
                    return Ty::Str;
                }
                for a in args {
                    let at = self.check_expr(a);
                    if at != Ty::Int64 && at != Ty::Int32 && !ty_has_error(&at) {
                        self.err(
                            "E0201",
                            a.span,
                            format!("`slice` expects Int bounds, found {}", at.name()),
                            None,
                        );
                    }
                }
                Ty::Str
            }
            "parse_int" | "parse_float" => {
                if !args.is_empty() {
                    self.err(
                        "E0201",
                        span,
                        format!("`{}` takes no arguments", method),
                        None,
                    );
                }
                let t = if method == "parse_int" {
                    Ty::Int64
                } else {
                    Ty::Float64
                };
                let key = Self::inst_key("Option", std::slice::from_ref(&t));
                self.generic_insts
                    .insert(key, ("Option".to_string(), vec![t.clone()]));
                opt_of(t)
            }
            _ => {
                self.err(
                    "E0401",
                    method_span,
                    format!(
                        "unknown method `{}` (available on String: split, trim, clone, len, byte_at, slice, parse_int, parse_float)",
                        method
                    ),
                    None,
                );
                Ty::Error
            }
        }
    }

    /// J5 `Rng` methods. The generator mutates through the receiver pointer
    /// (same model as `Vec::push`), so calls advance the caller's stream.
    fn check_rng_method(
        &mut self,
        span: Span,
        method: &str,
        method_span: Span,
        args: &[Expr],
    ) -> Ty {
        match method {
            "next_f64" => {
                if !args.is_empty() {
                    self.err("E0201", span, "`next_f64` takes no arguments", None);
                }
                Ty::Float64
            }
            "next_int" => {
                if args.len() != 1 {
                    self.err("E0201", span, "`next_int` takes exactly 1 argument", None);
                    return Ty::Int64;
                }
                let at = self.check_expr(&args[0]);
                if at != Ty::Int64 && at != Ty::Int32 && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        args[0].span,
                        format!("`next_int` expects an integer bound, found {}", at.name()),
                        None,
                    );
                }
                Ty::Int64
            }
            _ => {
                self.err(
                    "E0401",
                    method_span,
                    format!(
                        "unknown method `{}` (available on Rng: next_f64, next_int)",
                        method
                    ),
                    None,
                );
                Ty::Error
            }
        }
    }

    /// J2 file/CSV/JSON builtins (C-backed; row/offset-attributed `String`
    /// errors, so no new error types are needed).
    fn io_result_int(&mut self) -> Ty {
        let t = Ty::Generic {
            base: "Result".to_string(),
            args: vec![Ty::Int64, Ty::Str],
        };
        self.record_ty(&t);
        t
    }

    fn check_io_builtin(&mut self, span: Span, name: &str, args: &[Expr]) -> Ty {
        let rows_t = || Ty::Generic {
            base: "Vec".to_string(),
            args: vec![Ty::Generic {
                base: "Vec".to_string(),
                args: vec![Ty::Str],
            }],
        };
        let str_res = || Ty::Generic {
            base: "Result".to_string(),
            args: vec![Ty::Str, Ty::Str],
        };
        let rows_res = || Ty::Generic {
            base: "Result".to_string(),
            args: vec![rows_t(), Ty::Str],
        };
        let want = match name {
            "read_file" | "csv_bad_rows" | "json_rows" => 1,
            _ => 2,
        };
        if args.len() != want {
            self.err(
                "E0201",
                span,
                format!(
                    "`{}` takes exactly {} argument(s), found {}",
                    name,
                    want,
                    args.len()
                ),
                None,
            );
            return if name == "csv_bad_rows" {
                Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Int64],
                }
            } else if name == "csv_rows" || name == "json_rows" {
                rows_res()
            } else {
                str_res()
            };
        }
        let expect = |this: &mut Self, i: usize, want: &Ty, what: &str| {
            let at = this.check_expr(&args[i]);
            if &at != want && !ty_has_error(&at) {
                this.err(
                    "E0201",
                    args[i].span,
                    format!(
                        "`{}` expects {} {}, found {}",
                        name,
                        what,
                        want.name(),
                        at.name()
                    ),
                    None,
                );
            }
        };
        let t = match name {
            "read_file" => {
                expect(self, 0, &Ty::Str, "path");
                str_res()
            }
            "csv_rows" => {
                expect(self, 0, &Ty::Str, "content");
                expect(self, 1, &Ty::Bool, "strict flag");
                rows_res()
            }
            "csv_bad_rows" => {
                expect(self, 0, &Ty::Str, "content");
                Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Int64],
                }
            }
            _ => {
                expect(self, 0, &Ty::Str, "content");
                rows_res()
            }
        };
        self.record_ty(&t);
        t
    }

    /// Whether a value can be `println`-ed: primitives plus `Vec` /
    /// `Option` / `Result` / `Missing` nesting over printable types
    /// (structs, enums, and references stay E0201 until stdlib `Display`
    /// in 0.2.0).
    fn printable(ty: &Ty) -> bool {
        match ty {
            Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str => true,
            Ty::Generic { base, args }
                if base == "Vec" || base == "Option" || base == "Missing" =>
            {
                args.len() == 1 && Self::printable(&args[0])
            }
            Ty::Generic { base, args } if base == "Result" => {
                args.len() == 2 && Self::printable(&args[0]) && Self::printable(&args[1])
            }
            Ty::Generic { base, args } if base == "Mat" => args.is_empty(),
            Ty::Generic { base, args } if base == "Factor" => args.is_empty(),
            _ => false,
        }
    }

    /// Record every composite instantiation inside a printed type so codegen
    /// emits `vfmt_`/`vprint_` helpers exactly for these (nested included).
    fn record_print_insts(ty: &Ty, out: &mut std::collections::HashSet<String>) {
        match ty {
            Ty::Generic { base, args }
                if base == "Vec" || base == "Option" || base == "Result" || base == "Missing" =>
            {
                out.insert(Self::inst_key(base, args));
                for a in args {
                    Self::record_print_insts(a, out);
                }
            }
            Ty::Generic { base, args } if base == "Mat" => {
                out.insert(Self::inst_key(base, args));
            }
            Ty::Generic { base, args } if base == "Factor" => {
                out.insert(Self::inst_key(base, args));
            }
            _ => {}
        }
    }

    fn check_call(&mut self, id: crate::ast::NodeId, span: Span, name: &str, args: &[Expr]) -> Ty {
        // `println` is the 0.1.0 output builtin (builtin dispatch, same
        // mechanism as the old `print`); `print` remains as a deprecated
        // alias (W1002). Both borrow their argument.
        if name == "println" || name == "print" {
            if name == "print" {
                self.warn("W1002", span, "`print` is deprecated; use `println`");
            }
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!("`{}` takes exactly 1 argument, found {}", name, args.len()),
                    None,
                );
                return Ty::Error;
            }
            let t = self.check_expr(&args[0]);
            if ty_has_error(&t) {
                // Poison from an already-reported error (e.g. unknown
                // variable): stay silent instead of cascading.
                return Ty::Error;
            }
            if !Self::printable(&t) {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`{}` supports Int32/Int64/Float64/Bool/String, Vec<T>, Option<T>, Result<T, E>, Missing<T>, Mat, Factor (of printable types); found {}",
                        name,
                        t.name()
                    ),
                    None,
                );
            } else {
                Self::record_print_insts(&t, &mut self.print_insts);
                // `Factor` printing nests `Vec<String>`/`Vec<Int32>`
                // formatters; record those layouts too (like `Mat`, the
                // outer key alone is not enough for codegen).
                if matches!(&t, Ty::Generic { base, args } if base == "Factor" && args.is_empty()) {
                    let sv = Ty::Generic {
                        base: "Vec".to_string(),
                        args: vec![Ty::Str],
                    };
                    let iv = Ty::Generic {
                        base: "Vec".to_string(),
                        args: vec![Ty::Int32],
                    };
                    Self::record_print_insts(&sv, &mut self.print_insts);
                    Self::record_print_insts(&iv, &mut self.print_insts);
                    self.record_vec_inst(&Ty::Str);
                    self.record_vec_inst(&Ty::Int32);
                }
            }
            return Ty::Unit;
        }
        // `to_string` (J1): total conversions to `String` for the four
        // printable scalar types. Floats format with full precision.
        if name == "to_string" {
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!("`to_string` takes exactly 1 argument, found {}", args.len()),
                    None,
                );
                return Ty::Str;
            }
            let t = self.check_expr(&args[0]);
            return match t {
                Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str => Ty::Str,
                _ if ty_has_error(&t) => Ty::Str,
                _ => {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`to_string` supports Int32/Int64/Float64/Bool/String, found {}",
                            t.name()
                        ),
                        None,
                    );
                    Ty::Str
                }
            };
        } else if name == "read_file"
            || name == "csv_rows"
            || name == "csv_bad_rows"
            || name == "json_rows"
        {
            return self.check_io_builtin(span, name, args);
        } else if name == "hash_str" {
            // K2: `hash_str(content): Int64` (FNV-1a over bytes).
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!("`hash_str` takes exactly 1 argument, found {}", args.len()),
                    None,
                );
                return Ty::Int64;
            }
            let at = self.check_expr(&args[0]);
            if at != Ty::Str && !ty_has_error(&at) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!("`hash_str` expects a String, found {}", at.name()),
                    None,
                );
            }
            return Ty::Int64;
        } else if name == "par_sum" {
            // K4: `par_sum(v: Vec<Float64>, nthreads: Int64): Float64`
            // (deterministic parallel blocked sum; see codegen).
            if args.len() != 2 {
                self.err(
                    "E0201",
                    span,
                    format!("`par_sum` takes exactly 2 arguments, found {}", args.len()),
                    None,
                );
                return Ty::Float64;
            }
            let at = self.check_expr(&args[0]);
            let want = Ty::Generic {
                base: "Vec".to_string(),
                args: vec![Ty::Float64],
            };
            if at != want && !ty_has_error(&at) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!("`par_sum` expects Vec<Float64>, found {}", at.name()),
                    None,
                );
            } else {
                self.record_vec_inst(&Ty::Float64);
            }
            let bt = self.check_expr(&args[1]);
            if bt != Ty::Int64 && bt != Ty::Int32 && !ty_has_error(&bt) {
                self.err(
                    "E0201",
                    args[1].span,
                    format!("`par_sum` expects an Int thread count, found {}", bt.name()),
                    None,
                );
            }
            return Ty::Float64;
        } else if name == "mat_zeros" || name == "mat_identity" {
            // M1: `mat_zeros(r, c)` / `mat_identity(n)` (Int args).
            let want_n = if name == "mat_zeros" { 2 } else { 1 };
            let mat_ty = Ty::Generic {
                base: "Mat".to_string(),
                args: vec![],
            };
            if args.len() != want_n {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`{}` takes exactly {} argument(s), found {}",
                        name,
                        want_n,
                        args.len()
                    ),
                    None,
                );
                return mat_ty;
            }
            for a in args {
                let at = self.check_expr(a);
                if at != Ty::Int64 && at != Ty::Int32 && !ty_has_error(&at) {
                    self.err(
                        "E0201",
                        a.span,
                        format!("`{}` expects Int dimensions, found {}", name, at.name()),
                        None,
                    );
                }
            }
            self.record_ty(&mat_ty);
            return mat_ty;
        } else if name == "mat_from_rows" {
            // M1: `mat_from_rows(Vec<Vec<Float64>>): Result<Mat, String>`
            // (ragged input is `Err`, never a trap).
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`mat_from_rows` takes exactly 1 argument, found {}",
                        args.len()
                    ),
                    None,
                );
                return Ty::Generic {
                    base: "Result".to_string(),
                    args: vec![
                        Ty::Generic {
                            base: "Mat".to_string(),
                            args: vec![],
                        },
                        Ty::Str,
                    ],
                };
            }
            let at = self.check_expr(&args[0]);
            let want = Ty::Generic {
                base: "Vec".to_string(),
                args: vec![Ty::Generic {
                    base: "Vec".to_string(),
                    args: vec![Ty::Float64],
                }],
            };
            if at != want && !ty_has_error(&at) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!(
                        "`mat_from_rows` expects Vec<Vec<Float64>>, found {}",
                        at.name()
                    ),
                    None,
                );
            } else {
                self.record_ty(&at);
            }
            let t = Ty::Generic {
                base: "Result".to_string(),
                args: vec![
                    Ty::Generic {
                        base: "Mat".to_string(),
                        args: vec![],
                    },
                    Ty::Str,
                ],
            };
            self.record_ty(&t);
            return t;
        } else if name == "factor_from_strings" {
            // F1: `factor_from_strings(Vec<String>): Factor` (levels in
            // first-appearance order; `""` inputs become code 0 = missing).
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`factor_from_strings` takes exactly 1 argument, found {}",
                        args.len()
                    ),
                    None,
                );
                let f = Ty::Generic {
                    base: "Factor".to_string(),
                    args: vec![],
                };
                self.record_ty(&f);
                return f;
            }
            let at = self.check_expr(&args[0]);
            let want = Ty::Generic {
                base: "Vec".to_string(),
                args: vec![Ty::Str],
            };
            if at != want && !ty_has_error(&at) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!(
                        "`factor_from_strings` expects Vec<String>, found {}",
                        at.name()
                    ),
                    None,
                );
            } else {
                self.record_ty(&at);
            }
            self.record_vec_inst(&Ty::Int32);
            let f = Ty::Generic {
                base: "Factor".to_string(),
                args: vec![],
            };
            self.record_ty(&f);
            return f;
        } else if name == "map" || name == "filter" || name == "fold" || name == "split_apply" {
            // C1 (Stage 1): functionals over NAMED functions (closures are
            // deferred; anything but a bare `fun` name is E0401). Element
            // and result types are scalars/String (first-order kernels).
            return self.check_functional(span, name, args);
        } else if name == "write_file" {
            // J6: `write_file(path, content): Result<Int64, String>`
            // (bytes written).
            if args.len() != 2 {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`write_file` takes exactly 2 arguments, found {}",
                        args.len()
                    ),
                    None,
                );
                return self.io_result_int();
            }
            let pt = self.check_expr(&args[0]);
            if pt != Ty::Str && !ty_has_error(&pt) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!("`write_file` expects a String path, found {}", pt.name()),
                    None,
                );
            }
            let ct = self.check_expr(&args[1]);
            if ct != Ty::Str && !ty_has_error(&ct) {
                self.err(
                    "E0201",
                    args[1].span,
                    format!("`write_file` expects String content, found {}", ct.name()),
                    None,
                );
            }
            return self.io_result_int();
        } else if name == "rng_seeded" {
            // J5: `rng_seeded(seed: Int64): Rng` (seed bits feed splitmix64).
            if args.len() != 1 {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "`rng_seeded` takes exactly 1 argument, found {}",
                        args.len()
                    ),
                    None,
                );
                return Ty::Generic {
                    base: "Rng".to_string(),
                    args: vec![],
                };
            }
            let at = self.check_expr(&args[0]);
            if at != Ty::Int64 && at != Ty::Int32 && !ty_has_error(&at) {
                self.err(
                    "E0201",
                    args[0].span,
                    format!("`rng_seeded` expects an integer seed, found {}", at.name()),
                    None,
                );
            }
            let t = Ty::Generic {
                base: "Rng".to_string(),
                args: vec![],
            };
            self.record_ty(&t);
            return t;
        } else if (name == "Some" || name == "Ok" || name == "Err" || name == "Present")
            && self.resolve_fun(name).is_none()
        {
            return self.check_ctor_call(id, span, name, args, None);
        }
        if (name == "None" || name == "Missing") && self.resolve_fun(name).is_none() {
            self.err(
                "E0201",
                span,
                format!("`{}` takes no parentheses; write `{}`", name, name),
                None,
            );
            return Ty::Error;
        }
        let fid_opt = self.resolve_fun(name);
        let (pt, rt) = match fid_opt.and_then(|fid| self.sigs.get(&fid)) {
            Some(s) => (s.0.clone(), s.1.clone()),
            None => {
                // Tuple-struct construction `Meters(1.0)` lives in value position too.
                let qtype = self.resolve_type(name);
                if let Some(def) = qtype.as_ref().and_then(|q| self.structs.get(q)).cloned() {
                    if !def.is_tuple {
                        self.err(
                            "E0201",
                            span,
                            format!(
                                "`{}` is a struct with named fields; use `{} {{ ... }}`",
                                name, name
                            ),
                            None,
                        );
                        return Ty::Struct(qtype.clone().unwrap_or_else(|| name.to_string()));
                    }
                    let q = qtype.unwrap_or_else(|| name.to_string());
                    return self.check_tuple_ctor(span, name, &def.fields, args, Ty::Struct(q));
                }
                if self
                    .resolve_type(name)
                    .as_ref()
                    .is_some_and(|q| self.enums.contains_key(q))
                {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "`{}` is an enum; construct a variant with `{}::Variant(..)`",
                            name, name
                        ),
                        None,
                    );
                    return Ty::Enum(self.resolve_type(name).unwrap_or_else(|| name.to_string()));
                }
                self.err(
                    "E0401",
                    span,
                    format!("unknown function `{}`", name),
                    Some("define it with `fun` before use".into()),
                );
                return Ty::Error;
            }
        };
        // Parameter + return dimensions (for unit conversion checks).
        let pdims: Vec<Dim> = fid_opt
            .and_then(|fid| self.param_dims.get(&fid))
            .cloned()
            .unwrap_or_default();
        let ret_dim: Dim = fid_opt
            .and_then(|fid| self.dims.get(&fid))
            .cloned()
            .unwrap_or_else(Dim::dimensionless);
        if args.len() != pt.len() {
            self.err(
                "E0201",
                span,
                format!(
                    "function `{}` takes {} arguments, found {}",
                    name,
                    pt.len(),
                    args.len()
                ),
                None,
            );
            return rt;
        }
        for (i, (a, p)) in args.iter().zip(pt.iter()).enumerate() {
            let saved_exp = self.expected.clone();
            self.expected = Some(p.clone());
            let at = match self.ctor_use(a) {
                Some((cn, cargs)) => self.check_ctor_call(a.id, a.span, &cn, &cargs, Some(p)),
                None => self.check_expr(a),
            };
            self.expected = saved_exp;
            // Units: argument dimension must match the parameter's
            // (literals pin; scales convert silently at codegen).
            if let Some(pdim) = pdims.get(i) {
                self.unify_dim(a, pdim, a.span, "argument");
            }
            // Reference compatibility: mutability + inner type must match;
            // elided lifetimes unify with anything (fresh inference at the
            // borrow); two explicit lifetimes must agree. A missing borrow
            // gets a targeted hint instead of a bare mismatch.
            let mut compat_ok = at == *p;
            let mut hint: Option<String> = None;
            match (p, &at) {
                (
                    Ty::Reference {
                        lifetime: pl,
                        mutable: pm,
                        inner: pi,
                    },
                    Ty::Reference {
                        lifetime: al,
                        mutable: am,
                        inner: ai,
                    },
                ) => {
                    let lts_ok = match (pl, al) {
                        (Some(x), Some(y)) => x == y,
                        _ => true,
                    };
                    compat_ok = pm == am && pi.as_ref() == ai.as_ref() && lts_ok;
                    if !compat_ok && hint.is_none() {
                        hint = Some("check mutability, inner type, and lifetimes".into());
                    }
                }
                (Ty::Reference { .. }, _) => {
                    compat_ok = false;
                    hint = Some("borrow the argument explicitly with `&`".into());
                }
                _ => {}
            }
            if !compat_ok && !ty_has_error(&at) && !ty_has_error(p) {
                self.err(
                    "E0201",
                    a.span,
                    format!(
                        "argument is {}, but parameter expects {}",
                        at.name(),
                        p.name()
                    ),
                    hint,
                );
            }
        }
        self.set_dim(id, ret_dim);
        rt
    }

    fn check_binary(&mut self, b: BinCheck<'_>) -> Ty {
        let id = b.id;
        let span = b.span;
        let op = b.op;
        let (lt, rt, ld, rd, lhs, rhs) = (b.lt, b.rt, b.ld, b.rd, b.lhs, b.rhs);
        // Poison from an already-reported error stays silent (no cascades);
        // the Error result propagates structurally via the clones below.
        if ty_has_error(lt) || ty_has_error(rt) {
            return Ty::Error;
        }
        // Dimension agreement for numeric operands (scales convert
        // silently at codegen; vectors must match or this is E0201).
        // A dimensionless *literal* side pins to the other side
        // (untyped constants coerce); dimensionless variables do not.
        let mut same_dim = |a: &Dim, b: &Dim| {
            if !a.same_dim(b) {
                self.err(
                    "E0201",
                    span,
                    format!(
                        "operator needs equal dimensions, found {} and {}",
                        a.display(),
                        b.display()
                    ),
                    Some("dimensions must agree; convert explicitly".into()),
                );
                false
            } else {
                true
            }
        };
        match op {
            BinOp::Add if *lt == Ty::Str && *rt == Ty::Str => {
                self.set_dim(id, Dim::dimensionless());
                Ty::Str
            }
            BinOp::Add | BinOp::Sub | BinOp::Rem => {
                if lt == rt && matches!(lt, Ty::Int32 | Ty::Int64 | Ty::Float64) {
                    let result = if ld.same_dim(rd) {
                        ld.clone()
                    } else if lhs.map(Self::is_dimless_literal).unwrap_or(false) {
                        self.set_dim(lhs.map(|e| e.id).unwrap_or(id), rd.clone());
                        rd.clone()
                    } else if rhs.map(Self::is_dimless_literal).unwrap_or(false) {
                        self.set_dim(rhs.map(|e| e.id).unwrap_or(id), ld.clone());
                        ld.clone()
                    } else {
                        same_dim(ld, rd);
                        ld.clone()
                    };
                    self.set_dim(id, result);
                    lt.clone()
                } else {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "operator needs two equal numeric types, found {} and {}",
                            lt.name(),
                            rt.name()
                        ),
                        Some(
                            "convert explicitly; Valvet has no implicit numeric conversions".into(),
                        ),
                    );
                    lt.clone()
                }
            }
            BinOp::Mul | BinOp::Div => {
                if lt == rt && matches!(lt, Ty::Int32 | Ty::Int64 | Ty::Float64) {
                    // Dimensions compose (`m/s * s` is `m`).
                    let d = if op == BinOp::Mul {
                        ld.mul(rd)
                    } else {
                        ld.div(rd)
                    };
                    self.set_dim(id, d);
                    lt.clone()
                } else {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "operator needs two equal numeric types, found {} and {}",
                            lt.name(),
                            rt.name()
                        ),
                        Some(
                            "convert explicitly; Valvet has no implicit numeric conversions".into(),
                        ),
                    );
                    lt.clone()
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if lt == rt
                    && matches!(lt, Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str)
                {
                    // Quantities compare only within one dimension;
                    // dimensionless literals pin to the other side.
                    if matches!(lt, Ty::Int32 | Ty::Int64 | Ty::Float64) && !ld.same_dim(rd) {
                        if lhs.map(Self::is_dimless_literal).unwrap_or(false) {
                            self.set_dim(lhs.map(|e| e.id).unwrap_or(id), rd.clone());
                        } else if rhs.map(Self::is_dimless_literal).unwrap_or(false) {
                            self.set_dim(rhs.map(|e| e.id).unwrap_or(id), ld.clone());
                        } else {
                            same_dim(ld, rd);
                        }
                    }
                    Ty::Bool
                } else if matches!(&lt, Ty::Generic { base, args } if base == "Missing" && args.len() == 1)
                    && lt == rt
                    && matches!(
                        &lt,
                        Ty::Generic { args, .. }
                            if matches!(&args[0], Ty::Int32 | Ty::Int64 | Ty::Float64 | Ty::Bool | Ty::Str)
                    )
                {
                    // `Missing == Missing` is true (unlike NaN); two
                    // `Present` values compare payloads with `==`, so NaN
                    // payloads stay unequal. Tags decide first.
                    Ty::Bool
                } else {
                    self.err(
                        "E0201",
                        span,
                        format!("cannot compare {} with {}", lt.name(), rt.name()),
                        None,
                    );
                    Ty::Bool
                }
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if lt == rt && matches!(lt, Ty::Int32 | Ty::Int64 | Ty::Float64) {
                    if !ld.same_dim(rd) {
                        if lhs.map(Self::is_dimless_literal).unwrap_or(false) {
                            self.set_dim(lhs.map(|e| e.id).unwrap_or(id), rd.clone());
                        } else if rhs.map(Self::is_dimless_literal).unwrap_or(false) {
                            self.set_dim(rhs.map(|e| e.id).unwrap_or(id), ld.clone());
                        } else {
                            same_dim(ld, rd);
                        }
                    }
                    Ty::Bool
                } else {
                    self.err(
                        "E0201",
                        span,
                        format!(
                            "ordered comparison needs two equal numeric types, found {} and {}",
                            lt.name(),
                            rt.name()
                        ),
                        None,
                    );
                    Ty::Bool
                }
            }
            BinOp::And | BinOp::Or => {
                if *lt == Ty::Bool && *rt == Ty::Bool {
                    Ty::Bool
                } else {
                    self.err(
                        "E0201",
                        span,
                        "logical operators need `Bool` operands",
                        None,
                    );
                    Ty::Bool
                }
            }
        }
    }

    fn insert(&mut self, id: NodeId, ty: Ty) -> Ty {
        self.types.insert(id, ty.clone());
        ty
    }

    /// Resolve an optional `@unit` annotation (dimensionless default).
    /// Unknown unit names are E0401; evaluation is total otherwise.
    fn eval_unit(&mut self, unit: &Option<UnitExpr>) -> Dim {
        match unit {
            None => Dim::dimensionless(),
            Some(u) => self.eval_unit_expr(u),
        }
    }

    /// Resolve a unit expression (unknown names are E0401).
    fn eval_unit_expr(&mut self, u: &UnitExpr) -> Dim {
        match crate::units::eval(u) {
            Ok(d) => d,
            Err((sp, msg)) => {
                self.err(
                    "E0401",
                    sp,
                    msg,
                    Some("see `std.units` for the known units".into()),
                );
                Dim::dimensionless()
            }
        }
    }

    /// Dimension of an expression (dimensionless when unrecorded).
    fn dim_of(&self, id: NodeId) -> Dim {
        self.dims
            .get(&id)
            .cloned()
            .unwrap_or_else(Dim::dimensionless)
    }

    /// Record one parameter dimension (called in signature order).
    fn param_dim_push(&mut self, fid: NodeId, dim: Dim) {
        self.param_dims.entry(fid).or_default().push(dim);
    }

    /// Whether an expression is a bare numeric literal (optionally
    /// negated): untyped constants coerce to any dimension, Go/Rust-style.
    /// Only syntactic literals qualify; dimensionless *variables* do not.
    fn is_dimless_literal(e: &Expr) -> bool {
        matches!(&e.kind, ExprKind::Int(_) | ExprKind::Float(_))
            || matches!(
                &e.kind,
                ExprKind::Unary {
                    op: UnOp::Neg,
                    expr,
                } if matches!(&expr.kind, ExprKind::Int(_) | ExprKind::Float(_))
            )
    }

    /// Record an expression's dimension.
    fn set_dim(&mut self, id: NodeId, dim: Dim) {
        self.dims.insert(id, dim);
    }

    /// Check expression `e` against declared dimension `want`: literals pin
    /// silently, same vectors pass, anything else is E0201 at `span`.
    fn unify_dim(&mut self, e: &Expr, want: &Dim, span: Span, ctx: &str) {
        let have = self.dim_of(e.id);
        if want.same_dim(&have) {
            return;
        }
        if Self::is_dimless_literal(e) {
            self.set_dim(e.id, want.clone());
            return;
        }
        self.err(
            "E0201",
            span,
            format!(
                "{}: dimension {} does not match {}",
                ctx,
                have.display(),
                want.display()
            ),
            Some("dimensions must agree; convert explicitly".into()),
        );
    }
}

/// D3 conversion table: int<->int (wrap), int<->Float64, Float64->int (saturate).
/// Everything involving Bool/String/Unit is rejected.
fn can_cast(from: &Ty, to: &Ty) -> bool {
    use Ty::*;
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (Int32, Int64)
            | (Int32, Float64)
            | (Int64, Int32)
            | (Int64, Float64)
            | (Float64, Int32)
            | (Float64, Int64)
    )
}
