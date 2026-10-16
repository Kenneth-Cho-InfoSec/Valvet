use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diagnostic;
use crate::sema::{is_copy_ty, Checked};

/// Valvet ownership + borrow checker (D4).
///
/// Move rules:
/// - `String` is owned and non-Copy: assignment-init, call argument (except
///   `print`, which borrows), and `return`/tail move ownership.
/// - `+` (concat), comparisons, and `print` borrow; they never move.
/// - Copy scalars (Int32/Int64/Float64/Bool) never move.
/// - Use-after-move is E0301. Reassignment (`var x = new`) resets move state.
/// - Branch-merge is conservative: moved in either `if` arm stays moved after.
///   Loop bodies are checked once; names moved in the body count as moved after
///   (documented conservative simplification).
///
/// Borrow rules (NLL-flavored, AST-level):
/// - `let r = &x` / `&mut x` creates a loan on x's place, held by r, live until
///   r's last use (precomputed per function; unused borrows die at once).
///   Temporaries (`f(&x)`) live through the enclosing statement (the call
///   is their last use).
/// - Shared loans coexist; any live `&mut` loan forbids all other uses of
///   overlapping places; any live loan forbids moves/writes of its place.
/// - Places overlap when roots match and paths are equal-or-prefix (field
///   precise: `&x.a` and `&mut x.b` coexist).
/// - Copying a borrow (`let s = r`) shares its region (loan duplicated).
/// - Returning/breaking a borrow out of a loop extends its loans to scope end.
/// - Moves out of borrowed content are rejected (E0301); writes through
///   `&`-held places are rejected (E0205/E0302); `&mut` roots need `var`.
/// - Intra-function lifetimes are trivially satisfied (all locals share the
///   body scope); boundaries are enforced by sema elision + dangling checks.
pub fn analyze(checked: &Checked) -> Vec<Diagnostic> {
    analyze_with_files(checked, &HashMap::new())
}

/// Ownership over a joint (multi-module) program: diagnostics attribute to
/// the owning module's file via `mod_files` (empty = single-source, unstamped).
pub fn analyze_with_files(
    checked: &Checked,
    mod_files: &HashMap<String, String>,
) -> Vec<Diagnostic> {
    let mut errs = vec![];
    for f in &checked.program.funs {
        let (last_use, seq_of, tail_of) = number_uses(&f.body);

        let mut cx = Cx {
            checked,
            errs: vec![],
            scopes: vec![HashMap::new()],
            loans: vec![],
            cur_module: f.module.clone(),
            last_use,
            seq_of,
            tail_of,
            cur: 0,
            loop_depth: 0,
            next_loan: 0,
        };
        // Parameters are initialized bindings.
        for p in &f.params {
            cx.declare(p.name.clone(), p.ty.clone());
        }
        cx.check_block(&f.body);
        // Attribute this function's diagnostics to its file (multi-file mode).
        if let Some(file) = mod_files.get(&f.module) {
            for d in cx.errs.iter_mut() {
                if d.file.is_none() {
                    d.file = Some(file.clone());
                }
            }
        }
        errs.append(&mut cx.errs);
    }
    errs
}

/// Pre-pass: number statements (+ block tails) in source order and record
/// each variable's last-use sequence. Conservative across control flow
/// (branches/loops share the linear numbering; sound over-approximation).
fn number_uses(
    b: &Block,
) -> (
    HashMap<String, usize>,
    HashMap<NodeId, usize>,
    HashMap<NodeId, usize>,
) {
    struct W {
        next: usize,
        last_use: HashMap<String, usize>,
        seq_of: HashMap<NodeId, usize>,
        tail_of: HashMap<NodeId, usize>,
    }
    impl W {
        fn note(&mut self, id: NodeId) {
            self.seq_of.insert(id, self.next);
            self.next += 1;
        }
        fn walk_block(&mut self, b: &Block) {
            for s in &b.stmts {
                self.walk_stmt(s);
            }
            // Tails execute after the block's statements; number + walk them.
            if let Some(t) = &b.tail {
                self.tail_of.insert(b.id, self.next);
                self.next += 1;
                self.walk_expr(t);
            }
        }
        fn walk_stmt(&mut self, s: &Stmt) {
            let id = stmt_id(s);
            self.note(id);
            match s {
                Stmt::Val { name, init, .. } | Stmt::Var { name, init, .. } => {
                    self.walk_expr(init);
                    self.last_use.remove(name);
                }
                Stmt::Expr { expr, .. } => self.walk_expr(expr),
                Stmt::If {
                    cond,
                    then_b,
                    else_b,
                    ..
                } => {
                    self.walk_expr(cond);
                    self.walk_block(then_b);
                    if let Some(b) = else_b {
                        self.walk_block(b);
                    }
                }
                Stmt::While { cond, body, .. } => {
                    self.walk_expr(cond);
                    self.walk_block(body);
                }
                Stmt::For {
                    var, iter, body, ..
                } => {
                    match iter {
                        ForIter::Range { lo, hi, .. } => {
                            self.walk_expr(lo);
                            self.walk_expr(hi);
                        }
                        ForIter::Each { vec } => self.walk_expr(vec),
                    }
                    self.last_use.remove(var);
                    self.walk_block(body);
                }
                Stmt::Loop { body, .. } => self.walk_block(body),
                Stmt::Break { value, .. } => {
                    if let Some(v) = value {
                        self.walk_expr(v);
                    }
                }
                Stmt::Continue { .. } => {}
                Stmt::Return { value, .. } => {
                    if let Some(v) = value {
                        self.walk_expr(v);
                    }
                }
                Stmt::Match {
                    scrutinee, arms, ..
                } => {
                    self.walk_expr(scrutinee);
                    for a in arms {
                        if let Some(g) = &a.guard {
                            self.walk_expr(g);
                        }
                        self.walk_pat(&a.pat);
                        self.walk_block(&a.body);
                    }
                }
                Stmt::LetElse {
                    scrutinee,
                    pat,
                    else_b,
                    ..
                } => {
                    // Bindings join the current scope (like `Val` without
                    // shadowing limits); the body continues after.
                    self.walk_expr(scrutinee);
                    self.walk_pat(pat);
                    self.walk_block(else_b);
                }
            }
            self.next += 1;
        }
        fn walk_pat(&mut self, p: &Pat) {
            match p {
                Pat::Bind(n, _) => {
                    self.last_use.remove(n);
                }
                Pat::Variant { sub, .. } => {
                    for x in sub {
                        self.walk_pat(x);
                    }
                }
                Pat::Struct { fields, .. } => {
                    for (_, s) in fields {
                        if let Some(x) = s {
                            self.walk_pat(x);
                        }
                    }
                }
                Pat::Or(alts) => {
                    for a in alts {
                        self.walk_pat(a);
                    }
                }
                _ => {}
            }
        }
        fn walk_expr(&mut self, e: &Expr) {
            match &e.kind {
                ExprKind::Var(n) => {
                    self.last_use.insert(n.clone(), self.next);
                }
                ExprKind::Call { args, .. } => {
                    for a in args {
                        self.walk_expr(a);
                    }
                }
                ExprKind::Method { receiver, args, .. } => {
                    self.walk_expr(receiver);
                    for a in args {
                        self.walk_expr(a);
                    }
                }
                ExprKind::Binary { lhs, rhs, .. } => {
                    self.walk_expr(lhs);
                    self.walk_expr(rhs);
                }
                ExprKind::Unary { expr, .. } => self.walk_expr(expr),
                ExprKind::Cast { expr, .. } => self.walk_expr(expr),
                ExprKind::AsUnit { expr, .. } => self.walk_expr(expr),
                ExprKind::AddrOf { expr, .. } => self.walk_expr(expr),
                ExprKind::Assign { target, value, .. } => {
                    Self::walk_place_static(target, &mut self.last_use, self.next);
                    Self::walk_place_idx(target, self);
                    self.walk_expr(value);
                }
                ExprKind::If {
                    cond,
                    then_b,
                    else_b,
                } => {
                    self.walk_expr(cond);
                    self.walk_block(then_b);
                    if let Some(b) = else_b {
                        self.walk_block(b);
                    }
                }
                ExprKind::Loop { body } => self.walk_block(body),
                ExprKind::Block(b) => self.walk_block(b),
                ExprKind::Match { scrutinee, arms } => {
                    self.walk_expr(scrutinee);
                    for a in arms {
                        if let Some(g) = &a.guard {
                            self.walk_expr(g);
                        }
                        self.walk_pat(&a.pat);
                        self.walk_block(&a.body);
                    }
                }
                ExprKind::Try { expr } => self.walk_expr(expr),
                ExprKind::StructLit { fields, .. } => {
                    for (_, _, fe) in fields {
                        self.walk_expr(fe);
                    }
                }
                ExprKind::Field { base, .. } => self.walk_expr(base),
                ExprKind::OptField { base, .. } => self.walk_expr(base),
                ExprKind::Index { base, idx } => {
                    self.walk_expr(base);
                    self.walk_expr(idx);
                }
                ExprKind::Variant { args, .. } => {
                    for a in args {
                        self.walk_expr(a);
                    }
                }
                ExprKind::VecMacro { elems } => {
                    for x in elems {
                        self.walk_expr(x);
                    }
                }
                ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
            }
        }
        fn walk_place_static(target: &Place, last_use: &mut HashMap<String, usize>, cur: usize) {
            match target {
                Place::Var(n, _) => {
                    last_use.insert(n.clone(), cur);
                }
                Place::Field { base, .. } => Self::walk_place_static(base, last_use, cur),
                Place::Deref { inner, .. } => Self::walk_place_static(inner, last_use, cur),
                Place::Index { base, .. } => Self::walk_place_static(base, last_use, cur),
            }
        }
        /// Index expressions inside places read like ordinary expressions.
        fn walk_place_idx(target: &Place, w: &mut W) {
            match target {
                Place::Var(_, _) => {}
                Place::Field { base, .. } => Self::walk_place_idx(base, w),
                Place::Deref { inner, .. } => Self::walk_place_idx(inner, w),
                Place::Index { base, idx, .. } => {
                    Self::walk_place_idx(base, w);
                    w.walk_expr(idx);
                }
            }
        }
    }
    fn stmt_id(s: &Stmt) -> NodeId {
        match s {
            Stmt::Val { id, .. }
            | Stmt::Var { id, .. }
            | Stmt::Expr { id, .. }
            | Stmt::If { id, .. }
            | Stmt::While { id, .. }
            | Stmt::For { id, .. }
            | Stmt::Loop { id, .. }
            | Stmt::Break { id, .. }
            | Stmt::Continue { id, .. }
            | Stmt::Return { id, .. }
            | Stmt::Match { id, .. }
            | Stmt::LetElse { id, .. } => *id,
        }
    }
    let mut w = W {
        next: 0,
        last_use: HashMap::new(),
        seq_of: HashMap::new(),
        tail_of: HashMap::new(),
    };
    w.walk_block(b);
    (w.last_use, w.seq_of, w.tail_of)
}

#[derive(Debug, Clone)]
struct VarState {
    ty: Ty,
    moved: bool,
    /// Byte span of the move, for the "value moved here" note on E0301.
    moved_span: Option<crate::source::Span>,
    /// Struct fields moved out individually (conservative: any field move
    /// forbids whole-value use; see `use_var`).
    moved_fields: HashSet<String>,
    /// Per-field move spans, parallel to `moved_fields`.
    moved_field_spans: HashMap<String, crate::source::Span>,
}

/// Move state snapshot for branch joins (if/else, match arms, loop bodies):
/// whole-moved vars plus per-var moved fields, so exclusive branches roll
/// back partial moves exactly like whole moves.
struct MoveSnap {
    vars: HashSet<String>,
    fields: HashMap<String, HashSet<String>>,
    /// Move spans parallel to `vars` (first-found wins at joins).
    spans: HashMap<String, crate::source::Span>,
    /// Move spans parallel to `fields`.
    field_spans: HashMap<String, HashMap<String, crate::source::Span>>,
}

/// A live borrow region.
#[derive(Debug, Clone)]
struct Loan {
    root: String,
    path: Vec<String>,
    mutable: bool,
    /// Holding variable (`None` = call temporary, lives through its statement).
    holder: Option<String>,
    /// Last-use statement seq of the holder (inclusive end of live range;
    /// creation statements always precede it).
    end: usize,
}

struct Cx<'a> {
    checked: &'a Checked,
    errs: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, VarState>>,
    loans: Vec<Loan>,
    /// Module of the function under analysis (for UFCS resolution).
    cur_module: String,
    /// Precomputed: variable -> last-use statement seq (per function).
    last_use: HashMap<String, usize>,
    /// Precomputed: statement/block-tail node id -> sequence number.
    seq_of: HashMap<NodeId, usize>,
    /// Precomputed: block id -> tail sequence number.
    tail_of: HashMap<NodeId, usize>,
    /// Current statement sequence (for loan ranges).
    cur: usize,
    /// Loop nesting depth (for break/return borrow escape).
    loop_depth: usize,
    next_loan: u32,
}

// Human-readable place for diagnostics (`x`, `x.a.b`).
fn describe_place(root: &str, path: &[String]) -> String {
    if path.is_empty() {
        root.to_string()
    } else {
        format!("{}.{}", root, path.join("."))
    }
}

/// Field type through a borrow (`&S` -> field type of `S`); `None` when
/// the root is not a borrowed struct with a known field.
fn borrowed_field_ty(ty: &Ty, field: &str, checked: &Checked) -> Option<Ty> {
    if let Ty::Reference { inner, .. } = ty {
        if let Ty::Struct(n) = inner.as_ref() {
            return checked
                .structs
                .get(n)
                .and_then(|d| d.fields.iter().find(|(f, _)| f == field))
                .map(|(_, t)| t.clone());
        }
    }
    None
}

/// Statement node id (mirrors the pre-pass helper).
fn stmt_id(s: &Stmt) -> NodeId {
    match s {
        Stmt::Val { id, .. }
        | Stmt::Var { id, .. }
        | Stmt::Expr { id, .. }
        | Stmt::If { id, .. }
        | Stmt::While { id, .. }
        | Stmt::For { id, .. }
        | Stmt::Loop { id, .. }
        | Stmt::Break { id, .. }
        | Stmt::Continue { id, .. }
        | Stmt::Return { id, .. }
        | Stmt::Match { id, .. }
        | Stmt::LetElse { id, .. } => *id,
    }
}

impl<'a> Cx<'a> {
    fn ty_of(&self, id: NodeId) -> Ty {
        self.checked.types.get(&id).cloned().unwrap_or(Ty::Unit)
    }

    /// Mirror of sema's `resolve_fun` (own module, then import aliases).
    fn resolve_fun(&self, name: &str, arity: usize) -> Option<NodeId> {
        let own = crate::resolver::qname(&self.cur_module, name);
        if let Some(id) = self.checked.fun_names.get(&(own, arity)) {
            return Some(*id);
        }
        if let Some((tm, tn)) = self
            .checked
            .aliases
            .get(&(self.cur_module.clone(), name.to_string()))
        {
            let tq = crate::resolver::qname(tm, tn);
            return self.checked.fun_names.get(&(tq, arity)).copied();
        }
        None
    }

    /// True exactly when sema's method-miss arm routes through UFCS: the
    /// receiver type has no methods and a free function takes it first
    /// (arity = explicit args + receiver).
    fn ufcs_hit(&self, method: &str, rt: &Ty, nargs: usize) -> bool {
        match rt {
            Ty::Generic { base, args } if base == "Vec" && args.len() == 1 => false,
            Ty::Generic { base, args } if base == "Mat" && args.is_empty() => false,
            Ty::Generic { base, args } if base == "Factor" && args.is_empty() => false,
            Ty::Str => false,
            Ty::Generic { base, args } if base == "Rng" && args.is_empty() => false,
            Ty::Error => false,
            _ => match self
                .resolve_fun(method, nargs + 1)
                .and_then(|fid| self.checked.sigs.get(&fid))
            {
                Some((pt, _)) => pt.first().is_some_and(|p| p == rt),
                None => false,
            },
        }
    }

    fn declare(&mut self, name: String, ty: Ty) {
        self.scopes.last_mut().unwrap().insert(
            name,
            VarState {
                ty,
                moved: false,
                moved_span: None,
                moved_fields: HashSet::new(),
                moved_field_spans: HashMap::new(),
            },
        );
    }

    fn copy_ty(&self, ty: &Ty) -> bool {
        is_copy_ty(ty, &self.checked.structs, &self.checked.enums)
    }

    /// Whether a value of this type moves in move position.
    fn moves_ty(&self, id: crate::ast::NodeId) -> bool {
        !is_copy_ty(&self.ty_of(id), &self.checked.structs, &self.checked.enums)
    }

    fn find(&self, name: &str) -> Option<VarState> {
        for s in self.scopes.iter().rev() {
            if let Some(v) = s.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn find_mut(&mut self, name: &str) -> Option<&mut VarState> {
        for s in self.scopes.iter_mut().rev() {
            if s.contains_key(name) {
                return s.get_mut(name);
            }
        }
        None
    }

    /// Do two places overlap? Same root and equal-or-prefix paths
    /// (field-precise: `x.a` and `x.b` are disjoint).
    fn places_overlap(r1: &str, p1: &[String], r2: &str, p2: &[String]) -> bool {
        if r1 != r2 {
            return false;
        }
        let a = p1.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        let b = p2.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        let n = a.len().min(b.len());
        a[..n] == b[..n]
    }

    /// Resolve a place expression to (root var, field path) for loan purposes.
    /// Dereferences resolve through the holder's single live loan; with zero
    /// or many live loans they fall back to the holder root itself
    /// (documented approximation: exact in all realistic cases).
    fn resolve_place(&self, target: &Place) -> (String, Vec<String>) {
        match target {
            // Reference-typed slots auto-deref: resolve through the holder's
            // single live loan into the referent region (mirrors sema).
            Place::Var(n, _) => match self.holder_region(n) {
                Some(region) => region,
                None => (n.clone(), vec![]),
            },
            Place::Field { base, field, .. } => {
                let (r, mut p) = self.resolve_place(base);
                p.push(field.clone());
                (r, p)
            }
            // Indices are runtime values: an element place overlaps the
            // whole vector (conservative; disjoint indices are not
            // distinguished).
            Place::Index { base, .. } => self.resolve_place(base),
            Place::Deref { inner, .. } => {
                // Reborrow through the holder's loan. The holder is the RAW
                // root variable (no auto-deref: `*mb` looks up loans held by
                // `mb`, not by the region `mb` already resolved to).
                let holder = match Self::raw_root(inner) {
                    Some(h) => h,
                    None => self.resolve_place(inner).0,
                };
                let mut regions = vec![];
                for l in &self.loans {
                    if l.holder.as_deref() == Some(holder.as_str()) && l.end >= self.cur {
                        regions.push((l.root.clone(), l.path.clone()));
                    }
                }
                if regions.len() == 1 {
                    regions.pop().unwrap()
                } else {
                    (holder, vec![])
                }
            }
        }
    }

    /// Index expressions inside a place (read in borrow position).
    fn place_indices<'p>(target: &'p Place, out: &mut Vec<&'p Expr>) {
        match target {
            Place::Index { base, idx, .. } => {
                Self::place_indices(base, out);
                out.push(idx);
            }
            Place::Field { base, .. } => Self::place_indices(base, out),
            Place::Deref { inner, .. } => Self::place_indices(inner, out),
            Place::Var(_, _) => {}
        }
    }

    /// Raw root variable of a place (no loan-region resolution).
    fn raw_root(target: &Place) -> Option<String> {
        match target {
            Place::Var(n, _) => Some(n.clone()),
            Place::Field { base, .. } => Self::raw_root(base),
            Place::Deref { inner, .. } => Self::raw_root(inner),
            Place::Index { base, .. } => Self::raw_root(base),
        }
    }

    /// Live loans overlapping a place, minus exempted holders.
    fn live_conflicts(&self, root: &str, path: &[String], exempt: &[String]) -> Vec<usize> {
        self.loans
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.end >= self.cur
                    && !exempt.iter().any(|h| Some(h) == l.holder.as_ref())
                    && Self::places_overlap(&l.root, &l.path, root, path)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Create a loan after checking conflicts. Returns false on error.
    /// `chain` holds vars in the borrow's own deref chain (reborrow freeze).
    fn create_loan(
        &mut self,
        root: String,
        path: Vec<String>,
        mutable: bool,
        holder: Option<String>,
        chain: &[String],
        span: crate::source::Span,
    ) -> bool {
        let mut bad_mut = false;
        let mut bad_shared = false;
        for i in self.live_conflicts(&root, &path, chain) {
            let l = &self.loans[i];
            if mutable || l.mutable {
                bad_mut = true;
            } else {
                bad_shared = true;
            }
        }
        let _ = bad_shared; // shared+shared always compatible
        if bad_mut {
            self.errs.push(
                Diagnostic::error(
                    "E0302",
                    span,
                    if mutable {
                        format!(
                            "cannot mutably borrow `{}` (already borrowed)",
                            describe_place(&root, &path)
                        )
                    } else {
                        format!(
                            "cannot borrow `{}` (already mutably borrowed)",
                            describe_place(&root, &path)
                        )
                    },
                )
                .with_help(
                    "a borrow ends at its last use; shorten it or clone the value".to_string(),
                ),
            );
            return false;
        }
        let end = match &holder {
            Some(h) => self.last_use.get(h).copied().unwrap_or(self.cur),
            // Temporaries (`f(&x)`): live through the enclosing statement
            // only (the call is their last use — real NLL; anything escaping
            // is caught by sema's dangling checks + region sharing).
            None => self.cur,
        };
        // Freeze: overlapped exempt loans extend over the newcomer.
        for i in self.live_conflicts(&root, &path, &[]) {
            let l = &self.loans[i];
            if chain.iter().any(|h| Some(h) == l.holder.as_ref()) {
                let e = end;
                self.loans[i].end = self.loans[i].end.max(e);
            }
        }
        let _ = self.next_loan;
        self.next_loan += 1;

        self.loans.push(Loan {
            root,
            path,
            mutable,
            holder,
            end,
        });
        true
    }

    /// Variables in a target's deref chain (writing through their loans).
    fn collect_target_holders(target: &Place, out: &mut Vec<String>) {
        match target {
            Place::Var(n, _) => {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
            Place::Field { base, .. } => Self::collect_target_holders(base, out),
            Place::Deref { inner, .. } => Self::collect_target_holders(inner, out),
            Place::Index { base, .. } => Self::collect_target_holders(base, out),
        }
    }

    /// Human-readable assignment target for diagnostics.
    fn describe_target(target: &Place) -> String {
        match target {
            Place::Var(n, _) => n.clone(),
            Place::Field { base, field, .. } => {
                format!("{}.{}", Self::describe_target(base), field)
            }
            Place::Deref { inner, .. } => format!("*{}", Self::describe_target(inner)),
            Place::Index { base, .. } => format!("{}[_]", Self::describe_target(base)),
        }
    }

    /// Reset move/borrow state overwritten by an assignment to a place:
    /// plain vars reset fully; field writes clear that field's move; borrow
    /// holders assigned over get their old loans truncated.
    fn reset_target(target: &Place, cx: &mut Cx) {
        match target {
            Place::Var(n, _) => {
                cx.truncate_holder(n);
                if let Some(v) = cx.find_mut(n) {
                    v.moved = false;
                    v.moved_fields.clear();
                }
            }
            Place::Field { base, field, .. } => {
                // Resolve root var for first-level field tracking.
                let mut root = base.as_ref();
                loop {
                    match root {
                        Place::Var(n, _) => {
                            if let Some(v) = cx.find_mut(n) {
                                // Only direct `v.f = ...` clears; deeper paths
                                // stay conservative.
                                if matches!(base.as_ref(), Place::Var(_, _)) {
                                    v.moved_fields.remove(field);
                                }
                            }
                            break;
                        }
                        Place::Field { base: b, .. } => root = b,
                        Place::Deref { .. } => break,
                        // Element writes keep base move state: the vector
                        // itself is neither moved nor restored.
                        Place::Index { .. } => break,
                    }
                }
            }
            // Element writes keep base move state (see above).
            Place::Index { .. } => {}
            Place::Deref { .. } => {}
        }
    }

    /// Truncate a holder's loans at reassignment (old borrow dies).
    fn truncate_holder(&mut self, holder: &str) {
        for l in self.loans.iter_mut() {
            if l.holder.as_deref() == Some(holder) && l.end > self.cur {
                l.end = self.cur;
            }
        }
    }

    /// Extend a holder's live loans to scope end (borrow escapes via `break`).
    fn escape_holder(&mut self, holder: &str) {
        for l in self.loans.iter_mut() {
            if l.holder.as_deref() == Some(holder) && l.end >= self.cur {
                l.end = usize::MAX;
            }
        }
    }

    fn use_var(&mut self, name: &str, span: crate::source::Span, move_ctx: bool) {
        let st = match self.find(name) {
            Some(s) => s,
            None => return, // unknown-name already reported by sema
        };
        // Any use of a place with a live MUTABLE loan is forbidden (Rust's
        // place rule — even Copy reads; exclusivity is about the place).

        let mut_live: Vec<usize> = self
            .live_conflicts(name, &[], &[])
            .into_iter()
            .filter(|i| self.loans[*i].mutable)
            .collect();
        if !mut_live.is_empty() {
            self.errs.push(
                Diagnostic::error(
                    "E0302",
                    span,
                    format!("cannot use `{}` while it is mutably borrowed", name),
                )
                .with_help("the borrow ends at its last use".to_string()),
            );
            return;
        }
        if self.copy_ty(&st.ty) {
            return;
        }
        if st.moved || !st.moved_fields.is_empty() {
            let mut d = Diagnostic::error("E0301", span, format!("use of moved value `{}`", name))
                .with_help(format!(
                    "clone before move, or stop using `{}` after it was moved",
                    name
                ));
            if let Some(ms) = st.moved_span {
                d = d.with_label(ms, "value moved here");
            } else {
                // Whole flag unset (join-merged field moves): label each
                // moved field, sorted for deterministic output.
                let mut fields: Vec<&String> = st.moved_fields.iter().collect();
                fields.sort();
                for f in fields {
                    if let Some(fs) = st.moved_field_spans.get(f) {
                        d = d.with_label(*fs, format!("field `{}` moved here", f));
                    }
                }
            }
            self.errs.push(d);
            return;
        }
        if move_ctx {
            if !self.live_conflicts(name, &[], &[]).is_empty() {
                self.errs.push(
                    Diagnostic::error(
                        "E0301",
                        span,
                        format!("cannot move `{}` while it is borrowed", name),
                    )
                    .with_help("end the borrow first (last use) or clone the value".to_string()),
                );
                return;
            }
            if let Some(v) = self.find_mut(name) {
                v.moved = true;
                v.moved_span = Some(span);
            }
        }
    }

    /// Field-level use `var.field`: whole-value and per-field move tracking.
    /// Non-Copy fields move in move position; Copy fields never move.
    fn use_field(&mut self, var: &str, field: &str, span: crate::source::Span, move_ctx: bool) {
        // Reads conflict with live MUTABLE loans on this field path.
        let path = vec![field.to_string()];
        let mut_live: Vec<usize> = self
            .live_conflicts(var, &path, &[])
            .into_iter()
            .filter(|i| self.loans[*i].mutable)
            .collect();
        if !mut_live.is_empty() {
            self.errs.push(
                Diagnostic::error(
                    "E0302",
                    span,
                    format!(
                        "cannot use `{}.{}` while it is mutably borrowed",
                        var, field
                    ),
                )
                .with_help("the borrow ends at its last use".to_string()),
            );
            return;
        }
        let st = match self.find(var) {
            Some(s) => s,
            None => return, // sema already reported
        };
        if st.moved {
            let mut d = Diagnostic::error("E0301", span, format!("use of moved value `{}`", var))
                .with_help("the whole value was moved earlier".to_string());
            if let Some(ms) = st.moved_span {
                d = d.with_label(ms, "value moved here");
            }
            self.errs.push(d);
            return;
        }
        if st.moved_fields.contains(field) {
            let mut d = Diagnostic::error(
                "E0301",
                span,
                format!("use of moved field `{}.{}`", var, field),
            )
            .with_help("the field was moved out earlier".to_string());
            if let Some(fs) = st.moved_field_spans.get(field) {
                d = d.with_label(*fs, "field moved here");
            }
            self.errs.push(d);
            return;
        }
        // Moving a non-Copy field out of a shared borrow is corruption
        // (shallow copy plus free); Copy fields (e.g. `f.nrows`) stay
        // legal, and reads stay legal — moves must clone first.
        if move_ctx {
            if let Ty::Reference { mutable: false, .. } = st.ty {
                let moves = match borrowed_field_ty(&st.ty, field, self.checked) {
                    Some(t) => !is_copy_ty(&t, &self.checked.structs, &self.checked.enums),
                    None => false,
                };
                if moves {
                    self.errs.push(
                        Diagnostic::error(
                            "E0301",
                            span,
                            format!("cannot move `{}.{}` out of borrowed content", var, field),
                        )
                        .with_help("clone the value out instead".to_string()),
                    );
                    return;
                }
            }
        }
        if !move_ctx {
            return;
        }
        // Moves conflict with any live loan on this field path.
        if !self.live_conflicts(var, &path, &[]).is_empty() {
            self.errs.push(
                Diagnostic::error(
                    "E0301",
                    span,
                    format!("cannot move `{}.{}` while it is borrowed", var, field),
                )
                .with_help("end the borrow first (last use) or clone the value".to_string()),
            );
            return;
        }
        // Resolve the field type to decide whether this use moves.
        let fty = match &st.ty {
            Ty::Struct(n) => self
                .checked
                .structs
                .get(n)
                .and_then(|d| d.fields.iter().find(|(f, _)| f == field))
                .map(|(_, t)| t.clone()),
            _ => None,
        };
        match fty {
            Some(t) if !is_copy_ty(&t, &self.checked.structs, &self.checked.enums) => {
                if let Some(v) = self.find_mut(var) {
                    v.moved_fields.insert(field.to_string());
                    v.moved_field_spans.insert(field.to_string(), span);
                }
            }
            _ => {}
        }
    }

    fn moved_snapshot(&self) -> MoveSnap {
        let mut vars = HashSet::new();
        let mut fields: HashMap<String, HashSet<String>> = HashMap::new();
        let mut spans: HashMap<String, crate::source::Span> = HashMap::new();
        let mut field_spans: HashMap<String, HashMap<String, crate::source::Span>> = HashMap::new();
        for s in &self.scopes {
            for (n, v) in s {
                // Disjoint field borrows keep this legal under the borrow checker.
                if (v.moved || !v.moved_fields.is_empty())
                    && !is_copy_ty(&v.ty, &self.checked.structs, &self.checked.enums)
                {
                    vars.insert(n.clone());
                    if let Some(sp) = v.moved_span {
                        spans.entry(n.clone()).or_insert(sp);
                    }
                }
                if !v.moved_fields.is_empty() {
                    fields
                        .entry(n.clone())
                        .or_default()
                        .extend(v.moved_fields.iter().cloned());
                    for (f, sp) in &v.moved_field_spans {
                        field_spans
                            .entry(n.clone())
                            .or_default()
                            .entry(f.clone())
                            .or_insert(*sp);
                    }
                }
            }
        }
        MoveSnap {
            vars,
            fields,
            spans,
            field_spans,
        }
    }

    fn apply_moved(&mut self, set: &HashSet<String>, spans: &HashMap<String, crate::source::Span>) {
        for s in self.scopes.iter_mut() {
            for (n, v) in s.iter_mut() {
                if set.contains(n) {
                    v.moved = true;
                    if let Some(sp) = spans.get(n) {
                        v.moved_span = Some(*sp);
                    }
                }
            }
        }
    }

    /// Direct borrow binding: validate the place, check conflicts, and
    /// register a loan held by `holder` until its last use. Borrowing does not
    /// read the value (address-take only), so no use is recorded.
    fn check_borrow_init(
        &mut self,
        holder: &str,
        target: &Expr,
        mutable: bool,
        span: crate::source::Span,
    ) {
        let place = self.borrow_place(target);
        match place {
            None => {
                // Non-place (temporary): sema already rejected E0303.
            }
            Some((root, path)) => {
                if self.place_moved(&root, &path) {
                    self.errs.push(
                        Diagnostic::error(
                            "E0301",
                            span,
                            format!(
                                "cannot borrow moved value `{}`",
                                describe_place(&root, &path)
                            ),
                        )
                        .with_help("the value was moved earlier".to_string()),
                    );
                    return;
                }
                // Chain = vars in the borrow for reborrow freeze exemption.
                let mut chain = vec![];
                Self::collect_place_vars(target, &mut chain);
                self.create_loan(root, path, mutable, Some(holder.to_string()), &chain, span);
            }
        }
    }

    /// Whether a place is dead: whole var moved, or its first-level field moved.
    fn place_moved(&self, root: &str, path: &[String]) -> bool {
        match self.find(root) {
            None => false,
            Some(st) => {
                if st.moved {
                    return true;
                }
                match path.first() {
                    None => false,
                    Some(f) => st.moved_fields.contains(f),
                }
            }
        }
    }

    /// Place of a borrow target: (root var, field path), resolving derefs
    /// through holders' live loans. `None` for temporaries.
    /// The referent region of a reference-typed holder with exactly one live
    /// loan (`None` for non-references, params without loans, or ambiguous
    /// multi-loan holders — documented approximation).
    fn holder_region(&self, holder: &str) -> Option<(String, Vec<String>)> {
        let is_ref = match self.find(holder) {
            Some(st) => matches!(st.ty, Ty::Reference { .. }),
            None => false,
        };
        if !is_ref {
            return None;
        }
        let mut regions = vec![];
        for l in &self.loans {
            if l.holder.as_deref() == Some(holder) && l.end >= self.cur {
                regions.push((l.root.clone(), l.path.clone()));
            }
        }
        if regions.len() == 1 {
            regions.pop()
        } else {
            None
        }
    }

    fn borrow_place(&self, e: &Expr) -> Option<(String, Vec<String>)> {
        match &e.kind {
            // Auto-deref mirror of resolve_place (borrows of `m.y` land in
            // the referent region; `&m` itself is rejected by sema).
            ExprKind::Var(n) => Some(match self.holder_region(n) {
                Some(region) => region,
                None => (n.clone(), vec![]),
            }),
            ExprKind::Field { base, field, .. } => {
                let (r, mut p) = self.borrow_place(base)?;
                p.push(field.clone());
                Some((r, p))
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => {
                // Reborrow: resolve through the holder's live loan region.
                // Raw holder (no auto-deref — see resolve_place).
                let holder = match Self::raw_expr_root(expr) {
                    Some(h) => h,
                    None => self.borrow_place(expr)?.0,
                };
                let mut regions = vec![];
                for l in &self.loans {
                    if l.holder.as_deref() == Some(holder.as_str()) && l.end >= self.cur {
                        regions.push((l.root.clone(), l.path.clone()));
                    }
                }
                if regions.len() == 1 {
                    Some(regions.pop().unwrap())
                } else {
                    // No/singleton-unknown region: anchor at the holder.
                    Some((holder, vec![]))
                }
            }
            _ => None,
        }
    }

    /// Raw root variable of a borrow-target expression (no resolution).
    fn raw_expr_root(e: &Expr) -> Option<String> {
        match &e.kind {
            ExprKind::Var(n) => Some(n.clone()),
            ExprKind::Field { base, .. } => Self::raw_expr_root(base),
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => Self::raw_expr_root(expr),
            _ => None,
        }
    }

    /// Resolve the region a dereference reads: through the holder's live
    /// loan (exactly one), else anchored at the holder itself (params and
    /// dead borrows; documented approximation).
    fn deref_region(&self, e: &Expr) -> Option<(String, Vec<String>)> {
        match &e.kind {
            ExprKind::Var(n) => {
                let mut regions = vec![];
                for l in &self.loans {
                    if l.holder.as_deref() == Some(n.as_str()) && l.end >= self.cur {
                        regions.push((l.root.clone(), l.path.clone()));
                    }
                }
                if regions.len() == 1 {
                    Some(regions.pop().unwrap())
                } else {
                    Some((n.clone(), vec![]))
                }
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => self.deref_region(expr),
            ExprKind::Field { .. } => None,
            _ => None,
        }
    }

    /// All variable names mentioned in a place expression (reborrow chain).
    fn collect_place_vars(e: &Expr, out: &mut Vec<String>) {
        match &e.kind {
            ExprKind::Var(n) => {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
            ExprKind::Field { base, .. } => Self::collect_place_vars(base, out),
            ExprKind::Unary { expr, .. } => Self::collect_place_vars(expr, out),
            _ => {}
        }
    }

    /// Share live regions of borrow-typed source vars in an init expression
    /// with a new borrow-typed holder (ref-returning calls, copies). Call-arg
    /// temporaries (`id(&s)`) are shared too: their regions flow into the
    /// result per lifetime elision.
    fn share_call_borrows(&mut self, init: &Expr, holder: &str) {
        let mut srcs = vec![];
        Self::collect_ref_vars(init, self, &mut srcs);
        let end = self.last_use.get(holder).copied().unwrap_or(self.cur);
        for src in srcs {
            let mut extra = vec![];
            for l in &self.loans {
                if l.holder.as_deref() == Some(src.as_str()) && l.end >= self.cur {
                    let mut nl = l.clone();
                    nl.holder = Some(holder.to_string());
                    nl.end = nl.end.max(end);
                    extra.push(nl);
                }
            }
            self.loans.extend(extra);
        }
        // Temporaries created while checking this very init (call arguments)
        // also flow into a borrow-typed result.
        let mut temps = vec![];
        for l in &self.loans {
            if l.holder.is_none() && l.end >= self.cur {
                let mut nl = l.clone();
                nl.holder = Some(holder.to_string());
                nl.end = nl.end.max(end);
                temps.push(nl);
            }
        }
        self.loans.extend(temps);
    }

    /// Borrow-typed variable names anywhere inside an expression tree
    /// (including nested tails). Over-approximates result-region sources;
    /// sound direction for loan sharing.
    fn collect_ref_vars(e: &Expr, cx: &Cx, out: &mut Vec<String>) {
        match &e.kind {
            ExprKind::Var(n) => {
                if matches!(cx.ty_of(e.id), Ty::Reference { .. }) && !out.contains(n) {
                    out.push(n.clone());
                }
            }
            ExprKind::Call { args, .. } => {
                for a in args {
                    Self::collect_ref_vars(a, cx, out);
                }
            }
            ExprKind::Method { receiver, args, .. } => {
                Self::collect_ref_vars(receiver, cx, out);
                for a in args {
                    Self::collect_ref_vars(a, cx, out);
                }
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                Self::collect_ref_vars(lhs, cx, out);
                Self::collect_ref_vars(rhs, cx, out);
            }
            ExprKind::Unary { expr, .. } => Self::collect_ref_vars(expr, cx, out),
            ExprKind::Cast { expr, .. } => Self::collect_ref_vars(expr, cx, out),
            ExprKind::AsUnit { expr, .. } => Self::collect_ref_vars(expr, cx, out),
            ExprKind::AddrOf { expr, .. } => Self::collect_ref_vars(expr, cx, out),
            ExprKind::Assign { target, value, .. } => {
                Self::collect_place_refs(target, cx, out);
                Self::collect_ref_vars(value, cx, out);
            }
            ExprKind::If {
                cond,
                then_b,
                else_b,
            } => {
                Self::collect_ref_vars(cond, cx, out);
                Self::collect_block_refs(then_b, cx, out);
                if let Some(b) = else_b {
                    Self::collect_block_refs(b, cx, out);
                }
            }
            ExprKind::Loop { body } => Self::collect_block_refs(body, cx, out),
            ExprKind::Block(b) => Self::collect_block_refs(b, cx, out),
            ExprKind::Match { scrutinee, arms } => {
                Self::collect_ref_vars(scrutinee, cx, out);
                for a in arms {
                    if let Some(g) = &a.guard {
                        Self::collect_ref_vars(g, cx, out);
                    }
                    Self::collect_block_refs(&a.body, cx, out);
                }
            }
            ExprKind::Try { expr } => Self::collect_ref_vars(expr, cx, out),
            ExprKind::StructLit { fields, .. } => {
                for (_, _, fe) in fields {
                    Self::collect_ref_vars(fe, cx, out);
                }
            }
            ExprKind::Field { base, .. } => Self::collect_ref_vars(base, cx, out),
            ExprKind::Index { base, idx } => {
                Self::collect_ref_vars(base, cx, out);
                Self::collect_ref_vars(idx, cx, out);
            }
            ExprKind::OptField { base, .. } => Self::collect_ref_vars(base, cx, out),
            ExprKind::Variant { args, .. } => {
                for a in args {
                    Self::collect_ref_vars(a, cx, out);
                }
            }
            ExprKind::VecMacro { elems } => {
                for x in elems {
                    Self::collect_ref_vars(x, cx, out);
                }
            }
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
        }
    }

    fn collect_block_refs(b: &Block, cx: &Cx, out: &mut Vec<String>) {
        for s in &b.stmts {
            Self::collect_stmt_refs(s, cx, out);
        }
        if let Some(t) = &b.tail {
            Self::collect_ref_vars(t, cx, out);
        }
    }

    fn collect_stmt_refs(s: &Stmt, cx: &Cx, out: &mut Vec<String>) {
        match s {
            Stmt::Val { init, .. } | Stmt::Var { init, .. } => {
                Self::collect_ref_vars(init, cx, out)
            }
            Stmt::Expr { expr, .. } => Self::collect_ref_vars(expr, cx, out),
            Stmt::If {
                cond,
                then_b,
                else_b,
                ..
            } => {
                Self::collect_ref_vars(cond, cx, out);
                Self::collect_block_refs(then_b, cx, out);
                if let Some(b) = else_b {
                    Self::collect_block_refs(b, cx, out);
                }
            }
            Stmt::While { cond, body, .. } => {
                Self::collect_ref_vars(cond, cx, out);
                Self::collect_block_refs(body, cx, out);
            }
            Stmt::For { iter, body, .. } => {
                match iter {
                    ForIter::Range { lo, hi, .. } => {
                        Self::collect_ref_vars(lo, cx, out);
                        Self::collect_ref_vars(hi, cx, out);
                    }
                    ForIter::Each { vec } => Self::collect_ref_vars(vec, cx, out),
                }
                Self::collect_block_refs(body, cx, out);
            }
            Stmt::Loop { body, .. } => Self::collect_block_refs(body, cx, out),
            Stmt::Break { value, .. } => {
                if let Some(v) = value {
                    Self::collect_ref_vars(v, cx, out);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    Self::collect_ref_vars(v, cx, out);
                }
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                Self::collect_ref_vars(scrutinee, cx, out);
                for a in arms {
                    Self::collect_block_refs(&a.body, cx, out);
                }
            }
            Stmt::LetElse {
                scrutinee, else_b, ..
            } => {
                Self::collect_ref_vars(scrutinee, cx, out);
                Self::collect_block_refs(else_b, cx, out);
            }
        }
    }

    fn collect_place_refs(target: &Place, cx: &Cx, out: &mut Vec<String>) {
        // Places name variables but never yield borrow values in 0.1.0.
        let _ = (target, cx, out);
    }

    fn check_block(&mut self, b: &Block) {
        self.scopes.push(HashMap::new());
        for s in &b.stmts {
            self.check_stmt(s);
        }
        if let Some(t) = &b.tail {
            if let Some(seq) = self.tail_of.get(&b.id).copied() {
                self.cur = seq;
            }
            // Tail moves out of the function/block value.
            self.check_expr(t, true);
        }
        self.scopes.pop();
    }

    /// Check one match arm: bindings are fresh slots; owned payload bindings
    /// out of a variable scrutinee conservatively move the whole scrutinee.
    /// `move_ctx` propagates to arm tails (value position) like `if` tails.
    /// Check one arm from the pre-match move state, adding its moves to
    /// `union_out`. Arms are exclusive (like if/else branches), so each arm
    /// starts from `before`; the caller unions all arms and applies once
    /// after the match. Applying per-arm would poison later arms with
    /// earlier arms' moves (spurious E0301).
    fn check_match_arm(
        &mut self,
        arm: &MatchArm,
        scrut: &Expr,
        move_ctx: bool,
        union_out: &mut HashSet<String>,
        uspans_out: &mut HashMap<String, crate::source::Span>,
    ) {
        let before = self.moved_snapshot();
        let st = self.ty_of(scrut.id);
        let bindings = self.arm_bindings(&arm.pat, &st);
        // Moving an owned payload out of a variable scrutinee moves it (whole).
        let scrut_var = match &scrut.kind {
            ExprKind::Var(n) => Some(n.clone()),
            _ => None,
        };
        self.scopes.push(HashMap::new());
        for (name, ty) in &bindings {
            self.scopes.last_mut().unwrap().insert(
                name.clone(),
                VarState {
                    ty: ty.clone(),
                    moved: false,
                    moved_span: None,
                    moved_fields: HashSet::new(),
                    moved_field_spans: HashMap::new(),
                },
            );
        }
        if let Some(vn) = scrut_var {
            if bindings
                .iter()
                .any(|(_, t)| !is_copy_ty(t, &self.checked.structs, &self.checked.enums))
            {
                if let Some(v) = self.find_mut(&vn) {
                    v.moved = true;
                    v.moved_span = Some(scrut.span);
                }
            }
        }
        if let Some(g) = &arm.guard {
            self.check_expr(g, false);
        }
        for s in &arm.body.stmts {
            self.check_stmt(s);
        }
        if let Some(t) = &arm.body.tail {
            if let Some(seq) = self.tail_of.get(&arm.body.id).copied() {
                self.cur = seq;
            }
            self.check_expr(t, move_ctx);
        }
        self.scopes.pop();
        let moved_arm = self.moved_snapshot();
        self.restore_snapshot(&before);
        union_out.extend(moved_arm.vars);
        for (n, sp) in moved_arm.spans {
            uspans_out.entry(n).or_insert(sp);
        }
    }

    /// Binding slots introduced by a pattern: (name, type). Mirrors the sema
    /// traversal structurally (decoupled; see sema::check_pat). On any
    /// resolution failure returns what resolved so far (sema already errored).
    fn arm_bindings(&self, pat: &Pat, scrut_ty: &Ty) -> Vec<(String, Ty)> {
        match pat {
            Pat::Wild => vec![],
            Pat::LitInt(_) | Pat::LitFloat(_) | Pat::LitBool(_) | Pat::LitStr(_) => vec![],
            Pat::Bind(n, _) => vec![(n.clone(), scrut_ty.clone())],
            Pat::Variant {
                ty, variant, sub, ..
            } => {
                let ename = match ty {
                    Some(t) if t == "Option" || t == "Result" || t == "Missing" => t.clone(),
                    Some(t) => t.clone(),
                    None => match scrut_ty {
                        Ty::Enum(n) => n.clone(),
                        Ty::Generic { base, .. }
                            if base == "Option" || base == "Result" || base == "Missing" =>
                        {
                            base.clone()
                        }
                        _ => return vec![],
                    },
                };
                // Builtin generic payloads resolve structurally (mirrors
                // sema's check_pat; silent fallbacks: sema already errored).
                if ename == "Option" || ename == "Result" || ename == "Missing" {
                    let payload: Vec<Ty> = match (ename.as_str(), variant.as_str(), scrut_ty) {
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
                        _ => return vec![],
                    };
                    let mut out = vec![];
                    for (sp, pt) in sub.iter().zip(payload.iter()) {
                        out.extend(self.arm_bindings(sp, pt));
                    }
                    return out;
                }
                let def = match self.checked.enums.get(&ename) {
                    Some(d) => d,
                    None => return vec![],
                };
                let payload = match def.variants.iter().find(|(v, _)| v == variant) {
                    Some((_, p)) => p.clone(),
                    None => return vec![],
                };
                let mut out = vec![];
                for (sp, pt) in sub.iter().zip(payload.iter()) {
                    out.extend(self.arm_bindings(sp, pt));
                }
                out
            }
            Pat::Struct { name, fields } => {
                let def = match self.checked.structs.get(name) {
                    Some(d) => d,
                    None => return vec![],
                };
                let mut out = vec![];
                for (fname, sub) in fields {
                    let fty = match def.fields.iter().find(|(f, _)| f == fname) {
                        Some((_, t)) => t.clone(),
                        None => continue,
                    };
                    match sub {
                        None => out.push((fname.clone(), fty)),
                        Some(p) => out.extend(self.arm_bindings(p, &fty)),
                    }
                }
                out
            }
            Pat::Or(alts) => alts
                .first()
                .map(|a| self.arm_bindings(a, scrut_ty))
                .unwrap_or_default(),
        }
    }

    fn check_block_scoped(&mut self, b: &Block) {
        self.check_block(b);
    }

    fn check_stmt(&mut self, s: &Stmt) {
        if let Some(seq) = self.seq_of.get(&stmt_id(s)).copied() {
            self.cur = seq;
        }
        match s {
            Stmt::Val { name, init, ty, .. } | Stmt::Var { name, init, ty, .. } => {
                // Direct borrows (`let r = &x`) create held loans; other
                // borrow-typed inits duplicate live loans from source vars.
                if let ExprKind::AddrOf { expr, mutable } = &init.kind {
                    self.check_borrow_init(name, expr, *mutable, init.span);
                } else {
                    // RHS evaluated first (a use of another value moves it).
                    let moves = self.moves_ty(init.id);
                    self.check_expr(init, moves);
                    // Borrow-typed result (calls): share live regions of
                    // borrow-typed source vars in the init expression.
                    if matches!(self.ty_of(init.id), Ty::Reference { .. }) {
                        self.share_call_borrows(init, name);
                    }
                }
                let it = self.ty_of(init.id);
                let decl_ty = ty.clone().unwrap_or(it);
                self.scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarState {
                        ty: decl_ty,
                        moved: false,
                        moved_span: None,
                        moved_fields: HashSet::new(),
                        moved_field_spans: HashMap::new(),
                    },
                );
            }
            Stmt::Expr { expr, .. } => {
                self.check_expr(expr, false);
            }
            Stmt::If {
                cond,
                then_b,
                else_b,
                ..
            } => {
                self.check_expr(cond, false);
                let before = self.moved_snapshot();
                // then
                self.check_block_scoped(then_b);
                let moved_then = self.moved_snapshot();
                // restore, else
                self.restore_snapshot(&before);
                if let Some(e) = else_b {
                    self.check_block_scoped(e);
                }
                let moved_else = self.moved_snapshot();
                // union (spans: first-found wins, matching bool behavior)
                let mut union = moved_then.vars;
                union.extend(moved_else.vars);
                let mut uspans = moved_then.spans;
                for (n, sp) in moved_else.spans {
                    uspans.entry(n).or_insert(sp);
                }
                self.restore_snapshot(&before);
                self.apply_moved(&union, &uspans);
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond, false);
                let before = self.moved_snapshot();
                self.loop_depth += 1;
                self.check_block_scoped(body);
                self.loop_depth -= 1;
                let moved_body = self.moved_snapshot();
                self.restore_snapshot(&before);
                // Conservative: body moves persist after loop.
                self.apply_moved(&moved_body.vars, &moved_body.spans);
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                // Bounds/operand borrow; the loop variable is a fresh
                // immutable binding each iteration (owned clone for Vecs).
                let vt = match iter {
                    ForIter::Range { lo, hi, .. } => {
                        self.check_expr(lo, false);
                        self.check_expr(hi, false);
                        self.checked.types.get(&lo.id).cloned().unwrap_or(Ty::Unit)
                    }
                    ForIter::Each { vec } => {
                        self.check_expr(vec, false);
                        match self.ty_of(vec.id) {
                            Ty::Generic { args, .. } => args.first().cloned().unwrap_or(Ty::Unit),
                            _ => Ty::Unit,
                        }
                    }
                };
                let before = self.moved_snapshot();
                self.scopes.push(HashMap::new());
                self.scopes.last_mut().unwrap().insert(
                    var.clone(),
                    VarState {
                        ty: vt,
                        moved: false,
                        moved_span: None,
                        moved_fields: HashSet::new(),
                        moved_field_spans: HashMap::new(),
                    },
                );
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth -= 1;
                self.scopes.pop();
                let moved_body = self.moved_snapshot();
                self.restore_snapshot(&before);
                // Conservative like `while`: body moves persist after loop.
                self.apply_moved(&moved_body.vars, &moved_body.spans);
            }
            Stmt::Loop { body, .. } => {
                let before = self.moved_snapshot();
                self.loop_depth += 1;
                self.check_block_scoped(body);
                self.loop_depth -= 1;
                let moved_body = self.moved_snapshot();
                self.restore_snapshot(&before);
                self.apply_moved(&moved_body.vars, &moved_body.spans);
            }
            Stmt::Match {
                scrutinee, arms, ..
            } => {
                self.check_expr(scrutinee, false);
                let mut union = HashSet::new();
                let mut uspans: HashMap<String, crate::source::Span> = HashMap::new();
                for arm in arms {
                    self.check_match_arm(arm, scrutinee, false, &mut union, &mut uspans);
                }
                self.apply_moved(&union, &uspans);
            }
            Stmt::LetElse {
                scrutinee,
                pat,
                else_b,
                ..
            } => {
                // Success path continues inline (no arm join): check the
                // scrutinee first (it cannot be "after" its own move),
                // then declare bindings in the current scope; an owned
                // capture moves a variable scrutinee (mirrors
                // `check_match_arm`).
                self.check_expr(scrutinee, false);
                let st = self.ty_of(scrutinee.id);
                let bindings = self.arm_bindings(pat, &st);
                if let ExprKind::Var(n) = &scrutinee.kind {
                    if bindings
                        .iter()
                        .any(|(_, t)| !is_copy_ty(t, &self.checked.structs, &self.checked.enums))
                    {
                        if let Some(v) = self.find_mut(n) {
                            v.moved = true;
                            v.moved_span = Some(scrutinee.span);
                        }
                    }
                }
                for (name, ty) in &bindings {
                    self.declare(name.clone(), ty.clone());
                }
                for s in &else_b.stmts {
                    self.check_stmt(s);
                }
                if let Some(t) = &else_b.tail {
                    if let Some(seq) = self.tail_of.get(&else_b.id).copied() {
                        self.cur = seq;
                    }
                    self.check_expr(t, false);
                }
            }
            Stmt::Break { value, .. } => {
                // `break v` moves a value out of the loop (like `return`).
                if let Some(v) = value {
                    let moves = self.moves_ty(v.id);
                    self.check_expr(v, moves);
                    // Borrow-typed values escaping the loop keep their regions
                    // borrowed: extend holder loans to scope end.
                    if matches!(self.ty_of(v.id), Ty::Reference { .. }) {
                        let mut holders = vec![];
                        Self::collect_ref_vars(v, self, &mut holders);
                        for h in holders {
                            self.escape_holder(&h);
                        }
                    }
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    let moves = self.moves_ty(v.id);
                    self.check_expr(v, moves);
                }
            }
        }
    }

    fn restore_snapshot(&mut self, snap: &MoveSnap) {
        for s in self.scopes.iter_mut() {
            for (n, v) in s.iter_mut() {
                v.moved = snap.vars.contains(n);
                v.moved_span = snap.spans.get(n).copied();
                // Field moves must also roll back: exclusive branches must
                // not observe each other's partial moves (same rule as whole
                // moves above; previously leaked, causing spurious E0301).
                v.moved_fields = snap.fields.get(n).cloned().unwrap_or_default();
                v.moved_field_spans = snap.field_spans.get(n).cloned().unwrap_or_default();
            }
        }
    }

    /// `move_ctx`: whether a String-typed value in value position moves.
    fn check_expr(&mut self, e: &Expr, move_ctx: bool) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => {}
            ExprKind::Var(name) => self.use_var(
                name,
                e.span,
                move_ctx
                    && !is_copy_ty(
                        &self.ty_of(e.id),
                        &self.checked.structs,
                        &self.checked.enums,
                    ),
            ),
            ExprKind::Call { name, args } => {
                let borrow_only = name == "print"
                    || name == "println"
                    // Batch 5: `type_name` reads the checked type only.
                    || name == "type_name"
                    // C1 functionals borrow their data (kernels clone what
                    // they keep; fold clones owned inits at the boundary).
                    // F1 `par_map` borrows the vec (fresh output vector).
                    || name == "map"
                    || name == "filter"
                    || name == "fold"
                    || name == "par_map"
                    || name == "split_apply";
                for a in args {
                    let moves = !borrow_only && self.moves_ty(a.id);
                    self.check_expr(a, moves);
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                // All binary ops borrow (incl. String + and ==).
                let _ = op;
                self.check_expr(lhs, false);
                self.check_expr(rhs, false);
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => {
                // Holder use (Copy read) plus region read through the borrow.
                self.check_expr(expr, false);
                if let Some((root, path)) = self.deref_region(expr) {
                    let mut chain = vec![];
                    Self::collect_place_vars(expr, &mut chain);
                    let bad: Vec<usize> = self
                        .live_conflicts(&root, &path, &chain)
                        .into_iter()
                        .filter(|i| self.loans[*i].mutable)
                        .collect();
                    if !bad.is_empty() {
                        self.errs.push(
                            Diagnostic::error(
                                "E0302",
                                e.span,
                                format!(
                                    "cannot read `{}` while it is mutably borrowed",
                                    describe_place(&root, &path)
                                ),
                            )
                            .with_help("the borrow ends at its last use".to_string()),
                        );
                    }
                }
            }
            ExprKind::Unary { expr, .. } => self.check_expr(expr, false),
            ExprKind::Cast { expr, .. } => {
                // Conversions borrow (all convertible types are `Copy` anyway).
                self.check_expr(expr, false);
            }
            ExprKind::AsUnit { expr, .. } => {
                // Dimension assertion/conversion borrows (numerics are `Copy`).
                self.check_expr(expr, false);
            }
            ExprKind::Index { base, idx } => {
                // Element reads never move out (codegen clones owned
                // elements, exactly like `get`); both sides borrow.
                self.check_expr(base, false);
                self.check_expr(idx, false);
            }
            ExprKind::StructLit { fields, .. } => {
                // Literal construction consumes non-Copy field values (like args).
                for (_, _, fexpr) in fields {
                    let moves = !is_copy_ty(
                        &self.ty_of(fexpr.id),
                        &self.checked.structs,
                        &self.checked.enums,
                    );
                    self.check_expr(fexpr, moves);
                }
            }
            ExprKind::Field { base, field, .. } => {
                // Reading the base doesn't move it; the field itself follows
                // the ambient move context with per-field tracking.
                match &base.kind {
                    ExprKind::Var(name) => {
                        let n = name.clone();
                        let f = field.clone();
                        let sp = e.span;
                        self.check_expr(base, false);
                        self.use_field(&n, &f, sp, move_ctx);
                    }
                    ExprKind::Unary {
                        op: UnOp::Deref, ..
                    } => {
                        // Field through a borrow (`(*r).f`): resolve the full
                        // path and check conflicts (moves out are E0301 in
                        // sema, so only reads can validly arrive here).
                        if let Some((root, mut path)) = self.deref_region(base) {
                            path.push(field.clone());
                            let mut chain = vec![];
                            Self::collect_place_vars(base, &mut chain);
                            let bad: Vec<usize> = self
                                .live_conflicts(&root, &path, &chain)
                                .into_iter()
                                .filter(|i| self.loans[*i].mutable)
                                .collect();
                            if !bad.is_empty() {
                                self.errs.push(
                                    Diagnostic::error(
                                        "E0302",
                                        e.span,
                                        format!(
                                            "cannot use `{}` while it is mutably borrowed",
                                            describe_place(&root, &path)
                                        ),
                                    )
                                    .with_help("the borrow ends at its last use".to_string()),
                                );
                            }
                            if move_ctx {
                                self.errs.push(
                                    Diagnostic::error(
                                        "E0301",
                                        e.span,
                                        "cannot move out of borrowed content".to_string(),
                                    )
                                    .with_help("copy the value out instead".to_string()),
                                );
                            }
                        }
                    }
                    _ => {
                        // Field of a field chain (`l.inner.x`) or a temporary
                        // (`make_point().x`): walk to the root variable; a move
                        // out of a shared borrow is E0301 at any depth.
                        self.check_expr(base, false);
                        if move_ctx {
                            let mut root = base;
                            while let ExprKind::Field { base: b, .. } = &root.kind {
                                root = b;
                            }
                            if let ExprKind::Var(name) = &root.kind {
                                let bad_root = match self.find(name).map(|s| s.ty) {
                                    Some(rt) => match borrowed_field_ty(&rt, field, self.checked) {
                                        Some(ft)
                                            if !is_copy_ty(
                                                &ft,
                                                &self.checked.structs,
                                                &self.checked.enums,
                                            ) =>
                                        {
                                            matches!(rt, Ty::Reference { mutable: false, .. })
                                        }
                                        _ => false,
                                    },
                                    None => false,
                                };
                                if bad_root {
                                    self.errs.push(
                                        Diagnostic::error(
                                            "E0301",
                                            e.span,
                                            format!(
                                                "cannot move `{}` out of borrowed content",
                                                describe_place(name, std::slice::from_ref(field))
                                            ),
                                        )
                                        .with_help("clone the value out instead".to_string()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            ExprKind::Try { expr, .. } => {
                // `?` consumes the operand either way (success moves out,
                // failure moves into the early return).
                let moves = self.moves_ty(expr.id);
                self.check_expr(expr, moves);
            }
            ExprKind::VecMacro { elems } => {
                // Construction consumes non-Copy elements (like struct fields).
                for el in elems {
                    let moves = !is_copy_ty(
                        &self.ty_of(el.id),
                        &self.checked.structs,
                        &self.checked.enums,
                    );
                    self.check_expr(el, moves);
                }
            }
            ExprKind::Method {
                receiver,
                method,
                args,
                ..
            } => {
                if self.ufcs_hit(method, &self.ty_of(receiver.id), args.len()) {
                    // UFCS: the receiver is a real first argument and may
                    // move, exactly like the equivalent free call.
                    self.check_expr(receiver, self.moves_ty(receiver.id));
                    for a in args {
                        self.check_expr(a, self.moves_ty(a.id));
                    }
                    return;
                }
                // Receivers borrow (pointer model — never moves). Only `push`
                // moves its element argument in.
                self.check_expr(receiver, false);
                for a in args {
                    let moves = method == "push"
                        && !is_copy_ty(
                            &self.ty_of(a.id),
                            &self.checked.structs,
                            &self.checked.enums,
                        );
                    self.check_expr(a, moves);
                }
            }
            ExprKind::OptField { base, .. } => {
                // Optional access borrows the base; the fresh result is owned
                // by the caller context.
                self.check_expr(base, false);
            }
            ExprKind::Variant { args, .. } => {
                // Variant construction consumes non-Copy payloads (like args).
                for a in args {
                    let moves = !is_copy_ty(
                        &self.ty_of(a.id),
                        &self.checked.structs,
                        &self.checked.enums,
                    );
                    self.check_expr(a, moves);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.check_expr(scrutinee, false);
                let mut union = HashSet::new();
                let mut uspans: HashMap<String, crate::source::Span> = HashMap::new();
                for arm in arms {
                    self.check_match_arm(arm, scrutinee, move_ctx, &mut union, &mut uspans);
                }
                self.apply_moved(&union, &uspans);
            }
            ExprKind::Loop { body } => {
                // Value loop: check body in a scope; `break v` tails propagate
                // the move context like `if` tails.
                let before = self.moved_snapshot();
                self.scopes.push(HashMap::new());
                for s in &body.stmts {
                    self.check_stmt(s);
                }
                if let Some(t) = &body.tail {
                    if let Some(seq) = self.tail_of.get(&body.id).copied() {
                        self.cur = seq;
                    }
                    self.check_expr(t, false);
                }
                // NOTE: `break v` moves were recorded during stmt checks above;
                // a moved break value stays moved after (conservative union).
                self.scopes.pop();
                let moved_body = self.moved_snapshot();
                self.restore_snapshot(&before);
                self.apply_moved(&moved_body.vars, &moved_body.spans);
            }
            ExprKind::Block(b) => {
                // Desugared statement sequence: scoped like a value-loop
                // body; a tail (never present in our desugars) inherits
                // the ambient move context like `if` tails.
                let before = self.moved_snapshot();
                self.scopes.push(HashMap::new());
                for s in &b.stmts {
                    self.check_stmt(s);
                }
                if let Some(t) = &b.tail {
                    if let Some(seq) = self.tail_of.get(&b.id).copied() {
                        self.cur = seq;
                    }
                    self.check_expr(t, move_ctx);
                }
                self.scopes.pop();
                let moved_body = self.moved_snapshot();
                self.restore_snapshot(&before);
                self.apply_moved(&moved_body.vars, &moved_body.spans);
            }
            ExprKind::Assign { target, value, .. } => {
                let moves = self.moves_ty(value.id);
                self.check_expr(value, moves);
                // Index expressions read in borrow position; writing
                // through an index of a moved-away base is E0301.
                let mut idxs = vec![];
                Self::place_indices(target, &mut idxs);
                for ix in &idxs {
                    self.check_expr(ix, false);
                }
                // Resolve the target place and check write conflicts: any live
                // loan on an overlapping place forbids the write, except loans
                // held by vars in the target's own deref chain (writing
                // through the loan itself, e.g. `*r = v`).
                let (root, path) = self.resolve_place(target);
                if !idxs.is_empty() && self.place_moved(&root, &path) {
                    self.errs.push(
                        Diagnostic::error(
                            "E0301",
                            value.span,
                            format!(
                                "cannot assign into moved `{}`",
                                Self::describe_target(target)
                            ),
                        )
                        .with_help("the vector was moved away; clone it before moving".to_string()),
                    );
                }
                let mut chain = vec![];
                Self::collect_target_holders(target, &mut chain);
                let conflicts: Vec<usize> = self
                    .live_conflicts(&root, &path, &chain)
                    .into_iter()
                    .collect();
                if !conflicts.is_empty() {
                    self.errs.push(
                        Diagnostic::error(
                            "E0302",
                            value.span,
                            format!(
                                "cannot assign to `{}` while it is borrowed",
                                Self::describe_target(target)
                            ),
                        )
                        .with_help("the borrow ends at its last use".to_string()),
                    );
                }
                // Reassignment ends loans held by an overwritten borrow holder
                // and resets move state; borrow-typed values share regions.
                Self::reset_target(target, self);
                if matches!(self.ty_of(value.id), Ty::Reference { .. }) {
                    // Borrow-typed RHS: duplicate regions from source vars is
                    // handled by callers with known holders; here the target
                    // root (if a plain var) inherits live regions of source
                    // vars in the value (conservative sharing).
                    if let Place::Var(n, _) = target {
                        let mut srcs = vec![];
                        Self::collect_ref_vars(value, self, &mut srcs);
                        let end = self.last_use.get(n).copied().unwrap_or(self.cur);
                        for src in srcs {
                            let mut extra = vec![];
                            for l in &self.loans {
                                if l.holder.as_deref() == Some(src.as_str()) && l.end >= self.cur {
                                    let mut nl = l.clone();
                                    nl.holder = Some(n.clone());
                                    nl.end = nl.end.max(end);
                                    extra.push(nl);
                                }
                            }
                            self.loans.extend(extra);
                        }
                    }
                }
            }
            ExprKind::AddrOf { expr, mutable, .. } => {
                // Borrow outside a direct binding: a temporary living to scope
                // end (conservative). Borrowing takes an address, not a read.
                match self.borrow_place(expr) {
                    None => {} // sema E0303'd temporaries; total fallback
                    Some((root, path)) => {
                        if self.place_moved(&root, &path) {
                            self.errs.push(
                                Diagnostic::error(
                                    "E0301",
                                    e.span,
                                    format!(
                                        "cannot borrow moved value `{}`",
                                        describe_place(&root, &path)
                                    ),
                                )
                                .with_help("the value was moved earlier".to_string()),
                            );
                        } else {
                            let mut chain = vec![];
                            Self::collect_place_vars(expr, &mut chain);
                            self.create_loan(root, path, *mutable, None, &chain, e.span);
                        }
                    }
                }
            }
            ExprKind::If {
                cond,
                then_b,
                else_b,
            } => {
                self.check_expr(cond, false);
                let before = self.moved_snapshot();
                // Value-position if: both arms evaluated for value; check with move_ctx propagated
                // to tails by checking blocks manually.
                self.scopes.push(HashMap::new());
                for s in &then_b.stmts {
                    self.check_stmt(s);
                }
                if let Some(t) = &then_b.tail {
                    if let Some(seq) = self.tail_of.get(&then_b.id).copied() {
                        self.cur = seq;
                    }
                    self.check_expr(t, move_ctx);
                }
                self.scopes.pop();
                let moved_then = self.moved_snapshot();
                self.restore_snapshot(&before);
                if let Some(e) = else_b {
                    self.scopes.push(HashMap::new());
                    for s in &e.stmts {
                        self.check_stmt(s);
                    }
                    if let Some(t) = &e.tail {
                        if let Some(seq) = self.tail_of.get(&e.id).copied() {
                            self.cur = seq;
                        }
                        self.check_expr(t, move_ctx);
                    }
                    self.scopes.pop();
                }
                let moved_else = self.moved_snapshot();
                let mut union = moved_then.vars;
                union.extend(moved_else.vars);
                let mut uspans = moved_then.spans;
                for (n, sp) in moved_else.spans {
                    uspans.entry(n).or_insert(sp);
                }
                self.restore_snapshot(&before);
                self.apply_moved(&union, &uspans);
            }
        }
    }
}
