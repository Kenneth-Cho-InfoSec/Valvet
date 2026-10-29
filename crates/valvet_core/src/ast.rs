use crate::source::Span;

pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct Program {
    pub module: Option<String>,
    /// `import` declarations in source order (Phase H). In file mode the
    /// resolver stamps `from` with the importing module; single-source mode
    /// rejects non-empty imports (E0701 — imports need file compilation).
    pub imports: Vec<ImportDecl>,
    pub funs: Vec<FunDecl>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    /// Script mode: top-level statements of the entry file (empty for
    /// classic `fun main` programs and for imported modules). The resolver
    /// validates ownership and synthesizes `main` from these.
    pub top: Vec<Stmt>,
    /// Entry mode header (`mode imp;` / `mode exp;`); `Auto` when absent.
    /// `mode` is contextual (first two tokens only), never a keyword.
    pub mode: Mode,
    /// `prelude;` header (imp only): implicit data-scripting imports,
    /// injected by the resolver (see `inject_prelude`).
    pub prelude: bool,
    pub span: Span,
}

/// Entry mode: script (top-level statements run) vs strict classic
/// (`fun main` required, statements rejected). Default is `Auto`, which
/// behaves as imp; `mode imp;` is an explicit no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Auto,
    Imp,
    Exp,
}

/// One `import` declaration: `import a::b::load;`, `import a::b::{x, y as z};`.
/// `path` is the module path (`[a, b]`); `from` is the importing module
/// (`""` for the entry; stamped by the resolver, not the parser).
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub items: Vec<ImportItem>,
    pub from: String,
    pub span: Span,
}

/// One imported name: `load` or `load as ld` (`alias` is the local name).
#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: String,
    pub alias: Option<String>,
    pub span: Span,
}

impl ImportItem {
    /// Local (importing-module) name this item is bound as.
    pub fn local(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone)]
pub enum Item {
    Fun(FunDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Import(ImportDecl),
}

/// A struct declaration: named fields, tuple form, or unit (`struct Empty;`).
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub id: NodeId,
    pub name: String,
    pub name_span: Span,
    pub kind: StructKind,
    pub span: Span,
    /// Owning module (`""` for the entry / single-file mode; stamped by the resolver).
    pub module: String,
    /// `pub` visibility (cross-module access requires `pub`; default-private).
    pub is_pub: bool,
    /// `[cfg(feature = "...")]` gates (Batch 7, all must hold);
    /// empty always compiles.
    pub cfg: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum StructKind {
    Named(Vec<StructField>),
    Tuple(Vec<Ty>),
    Unit,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub span: Span,
    pub ty: Ty,
}

/// An enum declaration with tuple-payload or unit variants.
/// (Struct-variant syntax `V { f: T }` is parsed but rejected until Phase C.)
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub id: NodeId,
    pub name: String,
    pub name_span: Span,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
    /// Owning module (`""` for the entry / single-file mode; stamped by the resolver).
    pub module: String,
    /// `pub` visibility (cross-module access requires `pub`; default-private).
    pub is_pub: bool,
    /// `[cfg(feature = "...")]` gates (Batch 7, all must hold);
    /// empty always compiles.
    pub cfg: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub span: Span,
    pub payload: Vec<Ty>,
}

/// Resolved nominal-type definitions shared by sema/ownership/codegen.
#[derive(Debug, Clone)]
pub struct StructDef {
    pub fields: Vec<(String, Ty)>,
    pub is_tuple: bool,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    /// (variant name, payload types) in declaration order (= tag values).
    pub variants: Vec<(String, Vec<Ty>)>,
}

#[derive(Debug, Clone)]
pub struct FunDecl {
    pub id: NodeId,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub ret_span: Span,
    pub ret_unit: Option<UnitExpr>,
    pub body: Block,
    pub span: Span,
    /// Owning module (`""` for the entry / single-file mode; stamped by the resolver).
    pub module: String,
    /// `pub` visibility (cross-module calls require `pub`; default-private).
    pub is_pub: bool,
    /// `[cfg(feature = "...")]` gates (Batch 7, all must hold);
    /// empty always compiles.
    pub cfg: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub span: Span,
    pub ty: Ty,
    pub unit: Option<UnitExpr>,
}

/// A units-of-measure expression after `@` (`m/s^2`, `kg*m`, `(km/h)`).
/// Evaluated by `units.rs` into a dimension vector + scale.
#[derive(Debug, Clone)]
pub enum UnitExpr {
    Name(String, Span),
    Mul(Box<UnitExpr>, Box<UnitExpr>),
    Div(Box<UnitExpr>, Box<UnitExpr>),
    Pow(Box<UnitExpr>, i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int32,
    Int64,
    Float64,
    Bool,
    Str,
    Unit,
    /// Unresolved nominal type from the parser; sema resolves it to a
    /// primitive, struct, or enum (unknown names are E0201).
    Named(String),
    /// Resolved struct/enum types (only sema produces these).
    Struct(String),
    Enum(String),
    /// Builtin generic application: `Option<T>`, `Result<T, E>`, `Vec<T>`,
    /// and the other builtin containers. Zero-arity builtins such as
    /// `StringIndex` use an empty `args` list. Args are resolved recursively.
    Generic {
        base: String,
        args: Vec<Ty>,
    },
    /// Borrowed reference. `lifetime` is explicit (`'a`) or `None` (elided,
    /// resolved by sema's elision rules). No references inside structs,
    /// enums, or generic args in 0.1.0 (E0201 there).
    Reference {
        lifetime: Option<String>,
        mutable: bool,
        inner: Box<Ty>,
    },
    /// Poison from an already-reported error (stops cascade diagnostics;
    /// never user-visible when suppression works, never reaches codegen
    /// since any error aborts compilation). Behaves like `Unit` (Copy)
    /// in every predicate.
    Error,
}

impl Ty {
    pub fn is_copy(&self) -> bool {
        !matches!(self, Ty::Str)
    }
    pub fn name(&self) -> String {
        match self {
            Ty::Int32 => "Int32".to_string(),
            Ty::Int64 => "Int64".to_string(),
            Ty::Float64 => "Float64".to_string(),
            Ty::Bool => "Bool".to_string(),
            Ty::Str => "String".to_string(),
            Ty::Unit => "()".to_string(),
            Ty::Named(n) => format!("<unresolved {}>", n),
            Ty::Struct(n) | Ty::Enum(n) => n.clone(),
            Ty::Reference {
                lifetime,
                mutable,
                inner,
            } => {
                let mutbl = if *mutable { "mut " } else { "" };
                let lt = lifetime.as_deref().unwrap_or("_");
                format!("&'{}{} {}", lt, mutbl, inner.name())
            }
            Ty::Generic { base, args } => {
                if base == "StringIndex" && args.is_empty() {
                    return "StringIndex".to_string();
                }
                if let Some(rank) = array_rank(base) {
                    let inner: Vec<String> = args.iter().map(|a| a.name()).collect();
                    return format!("Array<{}, {}>", inner.join(", "), rank);
                }
                let inner: Vec<String> = args.iter().map(|a| a.name()).collect();
                format!("{}<{}>", base, inner.join(", "))
            }
            // Backstop only: suppression (item 2) keeps this out of output.
            Ty::Error => "<error>".to_string(),
        }
    }
    /// Display name, resolved or not.
    pub fn display(&self) -> String {
        self.name()
    }
}

/// Rank carried by the built-in compile-time-rank array base name.
/// `Array<T, 3>` is represented as `Ty::Generic { base: "Array3", args: [T] }`.
/// `ArrayD` and malformed names return `None`.
pub fn array_rank(base: &str) -> Option<usize> {
    let suffix = base.strip_prefix("Array")?;
    if suffix.len() != 1 {
        return None;
    }
    let rank: usize = suffix.parse().ok()?;
    (1..=8).contains(&rank).then_some(rank)
}

#[derive(Debug, Clone)]
pub struct Block {
    pub id: NodeId,
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Val {
        id: NodeId,
        name: String,
        name_span: Span,
        ty: Option<Ty>,
        unit: Option<UnitExpr>,
        init: Expr,
        span: Span,
    },
    Var {
        id: NodeId,
        name: String,
        name_span: Span,
        ty: Option<Ty>,
        unit: Option<UnitExpr>,
        init: Expr,
        span: Span,
    },
    Expr {
        id: NodeId,
        expr: Expr,
        span: Span,
    },
    If {
        id: NodeId,
        cond: Expr,
        then_b: Box<Block>,
        else_b: Option<Box<Block>>,
        span: Span,
    },
    Match {
        id: NodeId,
        scrutinee: Expr,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// `let PAT = expr else DIVERGING` (B3): bindings join the current
    /// scope; the `else` block must diverge (checked via `Flow`).
    LetElse {
        id: NodeId,
        scrutinee: Box<Expr>,
        pat: Pat,
        else_b: Box<Block>,
        span: Span,
    },
    While {
        id: NodeId,
        cond: Expr,
        body: Box<Block>,
        span: Span,
    },
    For {
        id: NodeId,
        var: String,
        var_span: Span,
        iter: ForIter,
        body: Box<Block>,
        span: Span,
    },
    Loop {
        id: NodeId,
        body: Box<Block>,
        span: Span,
    },
    Break {
        id: NodeId,
        value: Option<Expr>,
        span: Span,
    },
    Continue {
        id: NodeId,
        span: Span,
    },
    Return {
        id: NodeId,
        value: Option<Expr>,
        span: Span,
    },
}

/// Span of any statement (all variants carry one).
pub fn stmt_span(s: &Stmt) -> Span {
    match s {
        Stmt::Val { span, .. }
        | Stmt::Var { span, .. }
        | Stmt::Expr { span, .. }
        | Stmt::If { span, .. }
        | Stmt::While { span, .. }
        | Stmt::For { span, .. }
        | Stmt::Loop { span, .. }
        | Stmt::Break { span, .. }
        | Stmt::Continue { span, .. }
        | Stmt::Match { span, .. }
        | Stmt::LetElse { span, .. }
        | Stmt::Return { span, .. } => *span,
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

/// A match pattern (0.1.0: wildcards, literals, bindings, enum variants,
/// struct patterns, `|` alternatives; struct-variant patterns deferred).
#[derive(Debug, Clone)]
pub enum Pat {
    Wild,
    LitInt(i64),
    LitFloat(f64),
    LitBool(bool),
    LitStr(String),
    /// Binding (`x`). `_` parses to Wild, never Bind.
    Bind(String, Span),
    /// `Type::Variant(subs)` or bare `Variant(subs)` (resolved against the
    /// scrutinee enum type when bare).
    Variant {
        ty: Option<String>,
        variant: String,
        span: Span,
        sub: Vec<Pat>,
    },
    /// `Point { x, y: b }` — each entry binds or nests; `x` means `x: x`.
    Struct {
        name: String,
        fields: Vec<(String, Option<Pat>)>,
    },
    Or(Vec<Pat>),
}

/// What a `for` loop iterates: an integer range or a `Vec` (snapshot).
#[derive(Debug, Clone)]
pub enum ForIter {
    Range { lo: Expr, hi: Expr, inclusive: bool },
    Each { vec: Expr },
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Block,
    pub span: Span,
}

/// Associated-function call payload. Kept behind a `Box` so adding this
/// expression does not enlarge every `Expr`/`Stmt` in the AST.
#[derive(Debug, Clone)]
pub struct AssocCall {
    pub base: String,
    pub base_span: Span,
    pub type_args: Vec<Ty>,
    pub method: String,
    pub method_span: Span,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Var(String),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Assign {
        target: Place,
        op: AssignOp,
        value: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_b: Box<Block>,
        else_b: Option<Box<Block>>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Loop {
        body: Box<Block>,
    },
    /// Desugared statement sequence (`println(a, b)`, chained comparisons):
    /// parser-only sugar; our desugars never carry a tail.
    Block(Block),
    /// `expr?` — Result/Option error propagation (T-Prop).
    Try {
        expr: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        target: Ty,
        target_span: Span,
    },
    /// `expr as @unit` — assert/convert dimension, keeping the type.
    AsUnit {
        expr: Box<Expr>,
        unit: Box<UnitExpr>,
    },
    /// `Point { x: 1.0, y: 2.0 }` (colons; `=` is only for named call args).
    StructLit {
        name: String,
        name_span: Span,
        fields: Vec<(String, Span, Expr)>,
    },
    /// `&expr` / `&mut expr` — borrow (place expressions only).
    AddrOf {
        expr: Box<Expr>,
        mutable: bool,
    },
    /// `expr.field` — struct field access (methods stay E0800 until Phase F).
    Field {
        base: Box<Expr>,
        field: String,
        field_span: Span,
    },
    /// `base?.field` — optional payload field access; evaluates to
    /// `Option<FieldTy>` (`None` short-circuits).
    OptField {
        base: Box<Expr>,
        field: String,
        field_span: Span,
    },
    /// `vec![a, b, c]` — the sole builtin macro (homogeneous elements).
    VecMacro {
        elems: Vec<Expr>,
    },
    /// `vec![value; n]` — repeat a copyable value exactly `n` times.
    VecRepeat {
        value: Box<Expr>,
        len: usize,
    },
    /// `Vec<T>::with_capacity(n)` — associated constructor for a vector.
    AssocCall(Box<AssocCall>),
    /// `recv.method(args)` — method call (J1: `Vec` and `String` methods).
    Method {
        receiver: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<Expr>,
    },
    /// `v[i]` or `m[i, j]` — element read. The base must be a `Vec` (one
    /// index) or a `Mat` (two indices). Reads copy out; they never alias the
    /// buffer. The list form leaves room for N-dimensional arrays later.
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
    },
    /// `Shape::Circle(args)` — enum variant construction.
    Variant {
        ty: String,
        ty_span: Span,
        variant: String,
        var_span: Span,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    /// Prefix `*` dereference of a reference.
    Deref,
}

/// Assignment target: variable, field path on a variable root, or
/// dereference chain (`*r`, `**r`). Only variable-rooted places exist in
/// 0.1.0 (no temporaries).
#[derive(Debug, Clone)]
pub enum Place {
    Var(String, Span),
    Field {
        base: Box<Place>,
        field: String,
        span: Span,
    },
    Deref {
        inner: Box<Place>,
        span: Span,
    },
    /// `v[i]` or `m[i, j]` — element slot. Bounds are a runtime trap. Index
    /// count is checked against the base type in sema.
    Index {
        base: Box<Place>,
        indices: Vec<Expr>,
        span: Span,
    },
}

impl Place {
    /// Source span of the place itself (outermost layer).
    pub fn span(&self) -> Span {
        match self {
            Place::Var(_, span)
            | Place::Field { span, .. }
            | Place::Deref { span, .. }
            | Place::Index { span, .. } => *span,
        }
    }
}

/// Whether a type contains a reference anywhere inside (for the top-level
///-only rule: no nested references in 0.1.0).
pub fn has_ref(ty: &Ty) -> bool {
    match ty {
        Ty::Reference { .. } => true,
        Ty::Generic { args, .. } => args.iter().any(has_ref),
        Ty::Struct(_) | Ty::Enum(_) => false,
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
