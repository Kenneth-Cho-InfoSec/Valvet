use crate::ast::*;
use crate::diag::Diagnostic;
use crate::source::Span;
use crate::token::{Tok, Token};

/// Recursive-descent parser + precedence climbing. Deferred constructs are
/// lexed but rejected here with controlled E0800 (never silently accepted).
pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errs: Vec<Diagnostic>,
    next_id: NodeId,
    /// Struct literals forbidden here (condition/scrutinee positions where
    /// `{` opens a block — Rust's rule; parens re-allow, see LParen arm).
    no_struct: u32,
}

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Self {
            toks,
            pos: 0,
            errs: vec![],
            next_id: 1,
            no_struct: 0,
        }
    }

    pub fn parse(mut self) -> (Option<Program>, Vec<Diagnostic>) {
        self.skip_nls();
        // Optional `mode imp;` / `mode exp;` header (script vs strict entry
        // rules). Contextual: only the file's first two tokens count, so
        // `mode` stays a legal identifier everywhere else.
        let mut mode = Mode::Auto;
        if self.at_ident("mode")
            && matches!(
                self.toks.get(self.pos + 1).map(|t| &t.kind),
                Some(Tok::Ident(n)) if n == "imp" || n == "exp"
            )
        {
            let mspan = self.cur_span();
            self.bump();
            mode = if self.at_ident("exp") {
                Mode::Exp
            } else {
                Mode::Imp
            };
            self.bump();
            if self.expect_semi("mode header").is_none() {
                return (None, self.errs);
            }
            let _ = mspan;
            self.skip_nls();
        }
        // Optional `module a.b.c;`
        let mut module = None;
        if self.at_ident("module") {
            let start = self.cur_span().start;
            self.bump();
            let mut path = String::new();
            loop {
                match self.cur().clone() {
                    Tok::Ident(name) => {
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(&name);
                        self.bump();
                    }
                    _ => {
                        self.err("E0005", "expected module path segment");
                        break;
                    }
                }
                if matches!(self.cur(), Tok::Dot) {
                    self.bump();
                } else {
                    break;
                }
            }
            if self.expect_semi("module declaration").is_none() {
                return (None, self.errs);
            }
            let _ = start;
            module = Some(path);
            self.skip_nls();
        }
        let mut funs = vec![];
        let mut structs = vec![];
        let mut enums = vec![];
        let mut imports = vec![];
        let mut top = vec![];
        let prog_start = module
            .as_ref()
            .map(|_| 0)
            .unwrap_or_else(|| self.cur_span().start);
        while !matches!(self.cur(), Tok::Eof) {
            self.skip_nls();
            if matches!(self.cur(), Tok::Eof) {
                break;
            }
            let entry = self.pos;
            let saved_errs = self.errs.len();
            // Item-start is judged at the entry token (before the speculative
            // item parse moves `pos` into the broken construct).
            let entry_is_item = self.at_item_start();
            match self.parse_item() {
                Some(Item::Fun(f)) => funs.push(f),
                Some(Item::Struct(d)) => structs.push(d),
                Some(Item::Enum(d)) => enums.push(d),
                Some(Item::Import(i)) => imports.push(i),
                None => {
                    // Not an item: maybe a top-level script statement — but
                    // only when the token cannot start an item. A broken
                    // `fun`/`struct`/`enum`/`import`/etc must recover as a
                    // broken item (baseline); rewinding those to entry would
                    // re-parse their fragments as statements and swallow the
                    // real error (e.g. `if (x = 2)` inside a broken `fun`).
                    if entry_is_item {
                        self.recover();
                        if self.pos == entry {
                            // Recovery made no progress; force-advance to avoid stalling.
                            self.bump();
                        }
                        // Keep the item's own diagnostics (do not fall through
                        // to statement parsing).
                    } else {
                        // Rewind past the speculative item parse (its generic
                        // E0005 is discarded) and try a statement instead.
                        self.pos = entry;
                        self.errs.truncate(saved_errs);
                        let stmt_pos = self.pos;
                        let stmt_errs = self.errs.len();
                        match self.parse_stmt() {
                            Some(ParsedStmt::Stmt(s)) => {
                                if mode == Mode::Exp {
                                    self.err_at(
                                    "E0604",
                                    stmt_span(&s),
                                    "top-level statements need imp mode (this file declares `mode exp`)",
                                );
                                }
                                top.push(s);
                            }
                            Some(ParsedStmt::Bare(e, espan)) => {
                                if mode == Mode::Exp {
                                    self.err_at(
                                    "E0604",
                                    espan,
                                    "top-level statements need imp mode (this file declares `mode exp`)",
                                );
                                }
                                // Bare tail: consume a `;` like block bodies do;
                                // newlines are skipped by the loop head.
                                self.skip_nls();
                                if matches!(self.cur(), Tok::Semi) {
                                    self.bump();
                                }
                                top.push(Stmt::Expr {
                                    id: self.fresh(),
                                    expr: e,
                                    span: espan,
                                });
                            }
                            None => {
                                self.pos = stmt_pos;
                                self.errs.truncate(stmt_errs);
                                self.recover();
                                if self.pos == entry {
                                    // Recovery made no progress; force-advance to avoid stalling.
                                    self.bump();
                                }
                            }
                        }
                    }
                }
            }
            self.skip_nls();
        }
        let end = self.cur_span().end;
        if !self.errs.is_empty() {
            return (None, self.errs);
        }
        (
            Some(Program {
                module,
                imports,
                funs,
                structs,
                enums,
                top,
                mode,
                span: Span::new(prog_start, end),
            }),
            vec![],
        )
    }

    // ---------- items ----------

    fn parse_item(&mut self) -> Option<Item> {
        // attributes (deferred): skip `#[...]` with note
        while matches!(self.cur(), Tok::LBracket) {
            // ambiguous with indexing; at item level treat as attribute
            self.err("E0800", "attributes are not yet supported in Valvet 0.1.0");
            self.skip_balanced(Tok::LBracket, Tok::RBracket);
            self.skip_nls();
        }
        let is_pub = matches!(self.cur(), Tok::Pub);
        if is_pub {
            self.bump();
        }
        if matches!(self.cur(), Tok::Unsafe) {
            self.err("E0800", "`unsafe` is not yet supported in Valvet 0.1.0");
            self.bump();
        }
        match self.cur().clone() {
            Tok::Fun => self.parse_fun(is_pub).map(Item::Fun),
            Tok::Struct => self.parse_struct(is_pub).map(Item::Struct),
            Tok::Enum => self.parse_enum(is_pub).map(Item::Enum),
            Tok::Extern => {
                self.err(
                    "E0800",
                    "`extern` declarations are not yet supported in Valvet 0.1.0",
                );
                None
            }
            Tok::Trait
            | Tok::Impl
            | Tok::Type
            | Tok::Const
            | Tok::Static
            | Tok::Defer
            | Tok::Use
            | Tok::Where
            | Tok::When
            | Tok::Dyn
            | Tok::Move
            | Tok::Mut
            | Tok::Ref
            | Tok::In
            | Tok::Self_
            | Tok::SelfTy
            | Tok::Let => {
                self.err(
                    "E0800",
                    "this construct is not yet supported in Valvet 0.1.0",
                );
                None
            }
            Tok::Ident(w) if w == "import" => self.parse_import(is_pub).map(Item::Import),
            Tok::Ident(w) if w == "module" => {
                self.err(
                    "E0005",
                    "`module` must be the first declaration in the file",
                );
                None
            }
            _ => {
                let msg = format!("expected `fun` declaration, found {}", describe(self.cur()));
                self.err("E0005", msg);
                None
            }
        }
    }

    /// `import a::b::load;`, `import a::b::load as ld;`,
    /// `import a::b::{x, y as z};`. Dotted paths are E0602 (VR-002: dots only
    /// in `module` decls); globs are E0800; `pub import` (re-export) is E0800.
    fn parse_import(&mut self, is_pub: bool) -> Option<ImportDecl> {
        let start = self.cur_span().start;
        self.bump(); // import
        if is_pub {
            self.err(
                "E0800",
                "re-exports (`pub import`) are not yet supported in Valvet 0.1.0",
            );
        }
        let mut path = vec![];
        loop {
            match self.cur().clone() {
                Tok::Ident(name) => {
                    path.push(name);
                    self.bump();
                }
                _ => {
                    self.err("E0005", "expected module path segment in `import`");
                    return None;
                }
            }
            if matches!(self.cur(), Tok::ColonColon) {
                self.bump();
                // `import a::b::{x, y};` — brace list after `::`.
                if matches!(self.cur(), Tok::LBrace) {
                    return self.parse_import_braces(start, path);
                }
                if matches!(self.cur(), Tok::Star) {
                    self.err(
                        "E0800",
                        "glob imports (`*`) are not yet supported in Valvet 0.1.0",
                    );
                    return None;
                }
                continue;
            }
            break;
        }
        // Dots in imports are rejected (VR-002); suggest `::`.
        if matches!(self.cur(), Tok::Dot) {
            self.err(
                "E0602",
                "dotted import paths are rejected; use `::` separators (`import a::b::Item`)",
            );
            return None;
        }
        if path.len() < 2 {
            self.err(
                "E0005",
                "`import` needs a module path and an item (`import mod::Item`)",
            );
            return None;
        }
        let span = self.cur_span();
        let name = path.pop().unwrap();
        let mut alias = None;
        if matches!(self.cur(), Tok::As) {
            self.bump();
            match self.expect_ident("alias after `as`") {
                Some((a, _)) => alias = Some(a),
                None => return None,
            }
        }
        self.expect_separator("import declaration")?;
        let end = self.prev_span().end;
        Some(ImportDecl {
            path,
            items: vec![ImportItem { name, alias, span }],
            from: String::new(),
            span: Span::new(start, end),
        })
    }

    /// `{x, y as z}` tail of `import a::b::{...};` (`path` already parsed).
    fn parse_import_braces(&mut self, start: usize, path: Vec<String>) -> Option<ImportDecl> {
        self.bump(); // {
        let mut items = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RBrace) {
                self.bump();
                break;
            }
            if matches!(self.cur(), Tok::Star) {
                self.err(
                    "E0800",
                    "glob imports (`*`) are not yet supported in Valvet 0.1.0",
                );
                return None;
            }
            let (name, nspan) = self.expect_ident("imported name")?;
            let mut alias = None;
            if matches!(self.cur(), Tok::As) {
                self.bump();
                match self.expect_ident("alias after `as`") {
                    Some((a, _)) => alias = Some(a),
                    None => return None,
                }
            }
            items.push(ImportItem {
                name,
                alias,
                span: nspan,
            });
            self.skip_nls();
            match self.cur() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RBrace => {}
                _ => {
                    self.err("E0005", "expected `,` or `}` in import list");
                    return None;
                }
            }
        }
        if items.is_empty() {
            self.err("E0005", "`import {...}` needs at least one name");
            return None;
        }
        self.expect_separator("import declaration")?;
        let end = self.prev_span().end;
        Some(ImportDecl {
            path,
            items,
            from: String::new(),
            span: Span::new(start, end),
        })
    }

    fn parse_fun(&mut self, is_pub: bool) -> Option<FunDecl> {
        let start = self.cur_span().start;
        self.bump(); // fun
        let (name, name_span) = self.expect_ident("function name")?;
        if matches!(self.cur(), Tok::Lt) {
            self.err(
                "E0800",
                "generic functions are not yet supported in Valvet 0.1.0",
            );
            return None;
        }
        self.expect(Tok::LParen, "expected `(` after function name")?;
        let mut params = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RParen) {
                self.bump();
                break;
            }
            let (pn, ps) = self.expect_ident("parameter name")?;
            self.expect(Tok::Colon, "expected `:` after parameter name")?;
            let ty = self.parse_type()?;
            let mut unit = None;
            if matches!(self.cur(), Tok::At) {
                self.bump();
                unit = Some(self.parse_unit_expr()?);
            }
            params.push(Param {
                name: pn,
                span: ps,
                ty,
                unit,
            });
            match self.cur() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RParen => {}
                _ => {
                    self.err("E0005", "expected `,` or `)` in parameter list");
                    return None;
                }
            }
        }
        let mut ret = Ty::Unit;
        let mut ret_span = Span::empty(self.cur_span().start);
        let mut ret_unit = None;
        if matches!(self.cur(), Tok::Colon) {
            self.bump();
            ret_span = self.cur_span();
            ret = self.parse_type()?;
            if matches!(self.cur(), Tok::At) {
                self.bump();
                ret_unit = Some(self.parse_unit_expr()?);
            }
        }
        let body = self.parse_block()?;
        let end = body.span.end;
        Some(FunDecl {
            id: self.fresh(),
            name,
            name_span,
            params,
            ret,
            ret_span,
            ret_unit,
            body,
            span: Span::new(start, end),
            module: String::new(),
            is_pub,
        })
    }

    fn parse_struct(&mut self, is_pub: bool) -> Option<StructDecl> {
        let start = self.cur_span().start;
        self.bump(); // struct
        let (name, name_span) = self.expect_ident("struct name")?;
        if matches!(self.cur(), Tok::Lt) {
            self.err(
                "E0800",
                "generic structs are not yet supported in Valvet 0.1.0",
            );
            return None;
        }
        let kind = match self.cur() {
            Tok::LBrace => {
                self.bump();
                let mut fields = vec![];
                loop {
                    self.skip_nls();
                    if matches!(self.cur(), Tok::RBrace) {
                        self.bump();
                        break;
                    }
                    if matches!(self.cur(), Tok::Pub) {
                        self.bump();
                    }
                    let (fn_, fs) = self.expect_ident("field name")?;
                    self.expect(Tok::Colon, "expected `:` after field name")?;
                    let ty = self.parse_type()?;
                    fields.push(StructField {
                        name: fn_,
                        span: fs,
                        ty,
                    });
                    self.skip_nls();
                    match self.cur() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RBrace => {}
                        _ => {
                            self.err("E0005", "expected `,` or `}` in struct fields");
                            return None;
                        }
                    }
                }
                StructKind::Named(fields)
            }
            Tok::LParen => {
                self.bump();
                let mut tys = vec![];
                loop {
                    self.skip_nls();
                    if matches!(self.cur(), Tok::RParen) {
                        self.bump();
                        break;
                    }
                    tys.push(self.parse_type()?);
                    match self.cur() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RParen => {}
                        _ => {
                            self.err("E0005", "expected `,` or `)` in tuple struct fields");
                            return None;
                        }
                    }
                }
                self.expect(Tok::Semi, "expected `;` after tuple struct")?;
                StructKind::Tuple(tys)
            }
            Tok::Semi => {
                self.bump();
                StructKind::Unit
            }
            _ => {
                self.err("E0005", "expected `{`, `(...)`, or `;` after struct name");
                return None;
            }
        };
        let end = self.prev_span().end;
        Some(StructDecl {
            id: self.fresh(),
            name,
            name_span,
            kind,
            span: Span::new(start, end),
            module: String::new(),
            is_pub,
        })
    }

    fn parse_enum(&mut self, is_pub: bool) -> Option<EnumDecl> {
        let start = self.cur_span().start;
        self.bump(); // enum
        let (name, name_span) = self.expect_ident("enum name")?;
        if matches!(self.cur(), Tok::Lt) {
            self.err(
                "E0800",
                "generic enums are not yet supported in Valvet 0.1.0",
            );
            return None;
        }
        self.expect(Tok::LBrace, "expected `{` after enum name")?;
        let mut variants = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RBrace) {
                self.bump();
                break;
            }
            let (vn, vs) = self.expect_ident("variant name")?;
            let mut payload = vec![];
            if matches!(self.cur(), Tok::LParen) {
                self.bump();
                loop {
                    self.skip_nls();
                    if matches!(self.cur(), Tok::RParen) {
                        self.bump();
                        break;
                    }
                    payload.push(self.parse_type()?);
                    match self.cur() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RParen => {}
                        _ => {
                            self.err("E0005", "expected `,` or `)` in variant payload");
                            return None;
                        }
                    }
                }
            } else if matches!(self.cur(), Tok::LBrace) {
                self.err(
                    "E0800",
                    "struct-variant syntax is not yet supported in Valvet 0.1.0",
                );
                return None;
            }
            variants.push(EnumVariant {
                name: vn,
                span: vs,
                payload,
            });
            self.skip_nls();
            match self.cur() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RBrace => {}
                _ => {
                    self.err("E0005", "expected `,` or `}` in enum variants");
                    return None;
                }
            }
        }
        let end = self.prev_span().end;
        Some(EnumDecl {
            id: self.fresh(),
            name,
            name_span,
            variants,
            span: Span::new(start, end),
            module: String::new(),
            is_pub,
        })
    }

    /// Units-of-measure expression after `@`: `*`/`/` left-assoc, postfix
    /// `^n` (optional `-`), identifiers and parentheses. Anything else is
    /// E0005; unknown unit *names* are E0401 in sema.
    fn parse_unit_expr(&mut self) -> Option<UnitExpr> {
        self.skip_nls();
        let mut e = self.parse_unit_atom()?;
        loop {
            match self.cur() {
                Tok::Star => {
                    self.bump();
                    self.skip_nls();
                    let rhs = self.parse_unit_atom()?;
                    e = UnitExpr::Mul(Box::new(e), Box::new(rhs));
                }
                Tok::Slash => {
                    self.bump();
                    self.skip_nls();
                    let rhs = self.parse_unit_atom()?;
                    e = UnitExpr::Div(Box::new(e), Box::new(rhs));
                }
                _ => break,
            }
        }
        Some(e)
    }

    fn parse_unit_atom(&mut self) -> Option<UnitExpr> {
        self.skip_nls();
        let mut e = match self.cur().clone() {
            Tok::Ident(n) => {
                let sp = self.cur_span();
                self.bump();
                UnitExpr::Name(n, sp)
            }
            Tok::LParen => {
                self.bump();
                let inner = self.parse_unit_expr()?;
                if !matches!(self.cur(), Tok::RParen) {
                    self.err("E0005", "expected `)` in unit expression");
                    return None;
                }
                self.bump();
                inner
            }
            _ => {
                self.err("E0005", "expected a unit name after `@`");
                return None;
            }
        };
        // Postfix `^n` power with optional `-` (`s^2`, `s^-1`).
        while matches!(self.cur(), Tok::Caret) {
            self.bump();
            let neg = if matches!(self.cur(), Tok::Minus) {
                self.bump();
                true
            } else {
                false
            };
            match self.cur().clone() {
                Tok::IntLit(n) => {
                    self.bump();
                    let mag = (n as i32).saturating_abs();
                    let exp = if neg { mag.saturating_neg() } else { mag };
                    e = UnitExpr::Pow(Box::new(e), exp);
                }
                _ => {
                    self.err("E0005", "expected an integer exponent after `^`");
                    return None;
                }
            }
        }
        Some(e)
    }

    fn parse_type(&mut self) -> Option<Ty> {
        self.skip_nls();
        // `&T`, `&mut T`, `&'a T`, `&'a mut T` (nesting rejected in sema).
        if matches!(self.cur(), Tok::Amp | Tok::AmpMut) {
            let mutable = matches!(self.cur(), Tok::AmpMut);
            self.bump();
            self.skip_nls();
            let lifetime = match self.cur().clone() {
                Tok::Lifetime(n) => {
                    self.bump();
                    self.skip_nls();
                    Some(n)
                }
                _ => None,
            };
            // `&mut` as two tokens (`&` `mut`) is also accepted.
            let mut mutable = mutable;
            if !mutable && matches!(self.cur(), Tok::Mut) {
                self.bump();
                self.skip_nls();
                mutable = true;
            }
            let inner = self.parse_type()?;
            return Some(Ty::Reference {
                lifetime,
                mutable,
                inner: Box::new(inner),
            });
        }
        match self.cur().clone() {
            Tok::Ident(n) => {
                let base = n.clone();
                let ty = match n.as_str() {
                    "Int32" => Ty::Int32,
                    "Int64" => Ty::Int64,
                    "Float64" => Ty::Float64,
                    "Bool" => Ty::Bool,
                    "String" => Ty::Str,
                    "Unit" => Ty::Unit,
                    // Nominal struct/enum types resolve in sema (unknown names are E0201 there).
                    _ => Ty::Named(n),
                };
                let sp = self.cur_span();
                self.bump();
                // Generic application `Name<T, ...>` (only Option/Result/Vec
                // reach sema; anything else is rejected there).
                if matches!(self.cur(), Tok::Lt) {
                    self.bump();
                    let mut args = vec![];
                    loop {
                        self.skip_nls();
                        if matches!(self.cur(), Tok::Gt) {
                            self.bump();
                            break;
                        }
                        args.push(self.parse_type()?);
                        self.skip_nls();
                        match self.cur() {
                            Tok::Comma => {
                                self.bump();
                            }
                            Tok::Gt => {}
                            _ => {
                                self.err("E0005", "expected `,` or `>` in type arguments");
                                return None;
                            }
                        }
                    }
                    let _ = sp;
                    return Some(Ty::Generic { base, args });
                }
                // Reject [T] / fun(..) types (still deferred).
                if matches!(self.cur(), Tok::LBracket) {
                    self.err(
                        "E0800",
                        "slice and array types are not yet supported in Valvet 0.1.0",
                    );
                    return None;
                }
                Some(ty)
            }
            Tok::LParen => {
                // () unit only
                self.bump();
                self.skip_nls();
                if !matches!(self.cur(), Tok::RParen) {
                    self.err("E0800", "tuple types are not yet supported in Valvet 0.1.0");
                    return None;
                }
                self.bump();
                Some(Ty::Unit)
            }
            _ => {
                self.err(
                    "E0005",
                    format!("expected type, found {}", describe(self.cur())),
                );
                None
            }
        }
    }

    // ---------- blocks / statements ----------

    /// `match (scrutinee) { pat [if guard] -> body, ... }`.
    /// Arm bodies are blocks (parsed) or single expressions (wrapped).
    fn parse_match(&mut self) -> Option<(Expr, Vec<MatchArm>)> {
        self.expect(Tok::LParen, "expected `(` after `match`")?;
        let scrutinee = self.parse_expr()?;
        self.skip_nls();
        self.expect(Tok::RParen, "expected `)` after `match` scrutinee")?;
        self.skip_nls();
        self.expect(Tok::LBrace, "expected `{` for `match` arms")?;
        let mut arms = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RBrace) {
                self.bump();
                break;
            }
            if matches!(self.cur(), Tok::Eof) {
                self.err("E0005", "unclosed `match`: missing `}`");
                return None;
            }
            let asp = self.cur_span().start;
            let pat = self.parse_pat()?;
            let mut guard = None;
            if matches!(self.cur(), Tok::If) {
                self.bump();
                guard = Some(self.parse_expr()?);
            }
            self.expect(Tok::Arrow, "expected `->` after match pattern")?;
            self.skip_nls();
            let body = if matches!(self.cur(), Tok::LBrace) {
                self.parse_block()?
            } else {
                let e = self.parse_expr()?;
                let esp = e.span;
                Block {
                    id: self.fresh(),
                    stmts: vec![],
                    tail: Some(Box::new(e)),
                    span: esp,
                }
            };
            let end = body.span.end;
            arms.push(MatchArm {
                pat,
                guard,
                body,
                span: Span::new(asp, end),
            });
            self.skip_nls();
            // Arms separator: `,` preferred; newline before `}` or next arm works.
            if matches!(self.cur(), Tok::Comma) {
                self.bump();
            } else if matches!(self.cur(), Tok::RBrace) | matches!(self.cur(), Tok::Eof) {
                continue;
            } else {
                // Next arm on a fresh line without comma: peek an arm-start.
                // Anything else is a missing separator.
                match self.cur() {
                    Tok::RBrace | Tok::Eof => {}
                    _ => {
                        // Try to continue anyway if it looks like a pattern start;
                        // else report. Pattern starts: `_`, literal, ident.
                        match self.cur() {
                            Tok::Ident(_)
                            | Tok::IntLit(_)
                            | Tok::FloatLit(_)
                            | Tok::StrLit(_)
                            | Tok::CharLit(_)
                            | Tok::True
                            | Tok::False => {}
                            _ => {
                                self.err("E0004", "expected `,` between `match` arms");
                                return None;
                            }
                        }
                    }
                }
            }
        }
        Some((scrutinee, arms))
    }

    /// Parse one pattern (no top-level `|`; `parse_match` handles arms).
    fn parse_pat(&mut self) -> Option<Pat> {
        self.skip_nls();
        let head = self.parse_pat_atom()?;
        // `a | b` alternatives.
        if matches!(self.cur(), Tok::BitOr) {
            let mut alts = vec![head];
            while matches!(self.cur(), Tok::BitOr) {
                self.bump();
                alts.push(self.parse_pat_atom()?);
            }
            return Some(Pat::Or(alts));
        }
        Some(head)
    }

    fn parse_pat_atom(&mut self) -> Option<Pat> {
        self.skip_nls();
        let sp = self.cur_span();
        match self.cur().clone() {
            Tok::Ident(name) if name == "_" => {
                self.bump();
                Some(Pat::Wild)
            }
            Tok::IntLit(v) => {
                self.bump();
                Some(Pat::LitInt(v))
            }
            Tok::FloatLit(v) => {
                self.bump();
                Some(Pat::LitFloat(v))
            }
            Tok::StrLit(v) => {
                self.bump();
                Some(Pat::LitStr(v))
            }
            Tok::CharLit(_) => {
                self.err(
                    "E0800",
                    "character patterns are not yet supported in Valvet 0.1.0",
                );
                None
            }
            Tok::True => {
                self.bump();
                Some(Pat::LitBool(true))
            }
            Tok::False => {
                self.bump();
                Some(Pat::LitBool(false))
            }
            Tok::Ident(name) => {
                self.bump();
                // Constructors in patterns (`None`, `Some(..)`, ...) take
                // precedence over bindings, as in Rust/OCaml.
                if name == "None" || name == "Some" || name == "Ok" || name == "Err" {
                    let sub = if matches!(self.cur(), Tok::LParen) {
                        self.parse_pat_parens()?
                    } else {
                        vec![]
                    };
                    return Some(Pat::Variant {
                        ty: None,
                        variant: name,
                        span: sp,
                        sub,
                    });
                }
                // `Type::Variant(subs)` qualified form.
                if matches!(self.cur(), Tok::ColonColon) {
                    self.bump();
                    let (vname, _) = self.expect_ident("variant name")?;
                    // Unit variants take no parens; payload variants require them.
                    let sub = if matches!(self.cur(), Tok::LParen) {
                        self.parse_pat_parens()?
                    } else {
                        vec![]
                    };
                    return Some(Pat::Variant {
                        ty: Some(name),
                        variant: vname,
                        span: sp,
                        sub,
                    });
                }
                // `Name { f, g: p }` struct pattern.
                if matches!(self.cur(), Tok::LBrace) {
                    self.bump();
                    let mut fields = vec![];
                    loop {
                        self.skip_nls();
                        if matches!(self.cur(), Tok::RBrace) {
                            self.bump();
                            break;
                        }
                        let (fname, _) = self.expect_ident("field name")?;
                        let sub = if matches!(self.cur(), Tok::Colon) {
                            self.bump();
                            Some(self.parse_pat()?)
                        } else {
                            None
                        };
                        fields.push((fname, sub));
                        self.skip_nls();
                        match self.cur() {
                            Tok::Comma => {
                                self.bump();
                            }
                            Tok::RBrace => {}
                            _ => {
                                self.err("E0005", "expected `,` or `}` in struct pattern");
                                return None;
                            }
                        }
                    }
                    return Some(Pat::Struct { name, fields });
                }
                // `Variant(subs)` bare form or plain binding.
                if matches!(self.cur(), Tok::LParen) {
                    let sub = self.parse_pat_parens()?;
                    return Some(Pat::Variant {
                        ty: None,
                        variant: name,
                        span: sp,
                        sub,
                    });
                }
                Some(Pat::Bind(name, sp))
            }
            t => {
                self.err("E0005", format!("expected pattern, found {}", describe(&t)));
                None
            }
        }
    }

    /// Parenthesized expression (struct literals re-allowed inside).
    fn parse_paren_inner(&mut self, sp: Span) -> Option<Expr> {
        if matches!(self.cur(), Tok::RParen) {
            self.err(
                "E0800",
                "unit literal `()` is not yet supported in Valvet 0.1.0",
            );
            return None;
        }
        let e = self.parse_expr()?;
        self.skip_nls();
        self.expect(Tok::RParen, "expected `)`")?;
        Some(Expr {
            id: self.fresh(),
            span: Span::new(sp.start, self.prev_span().end),
            kind: e.kind,
        })
    }

    /// `(p0, p1)` sub-pattern list (also `()` for unit variants).
    fn parse_pat_parens(&mut self) -> Option<Vec<Pat>> {
        self.expect(Tok::LParen, "expected `(` for pattern arguments")?;
        let mut subs = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RParen) {
                self.bump();
                break;
            }
            subs.push(self.parse_pat()?);
            self.skip_nls();
            match self.cur() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RParen => {}
                _ => {
                    self.err("E0005", "expected `,` or `)` in pattern arguments");
                    return None;
                }
            }
        }
        Some(subs)
    }

    fn parse_block(&mut self) -> Option<Block> {
        self.skip_nls();
        let start = self.cur_span().start;
        self.expect(Tok::LBrace, "expected `{` to begin block")?;
        let id = self.fresh();
        let mut stmts = vec![];
        loop {
            self.skip_nls();
            if matches!(self.cur(), Tok::RBrace) {
                let end = self.cur_span().end;
                self.bump();
                return Some(Block {
                    id,
                    stmts,
                    tail: None,
                    span: Span::new(start, end),
                });
            }
            if matches!(self.cur(), Tok::Eof) {
                self.err("E0005", "unclosed block: missing `}`");
                return None;
            }
            // Tail expression? Try: if next tokens form `}` after an expression with no separator.
            // Strategy: parse statement; Stmt::Expr without trailing separator + `}` => convert to tail.
            let stmt_start = self.cur_span().start;
            match self.parse_stmt()? {
                ParsedStmt::Stmt(s) => stmts.push(s),
                ParsedStmt::Bare(e, espan) => {
                    self.skip_nls();
                    match self.cur() {
                        Tok::Semi => {
                            self.bump();
                            stmts.push(Stmt::Expr {
                                id: self.fresh(),
                                expr: e,
                                span: espan,
                            });
                        }
                        Tok::RBrace => {
                            let end = self.cur_span().end;
                            self.bump();
                            return Some(Block {
                                id,
                                stmts,
                                tail: Some(Box::new(e)),
                                span: Span::new(start, end),
                            });
                        }
                        Tok::Eof => {
                            self.err("E0005", "unclosed block: missing `}`");
                            return None;
                        }
                        _ => {
                            // newline-separated: NLs were skipped, so next stmt follows.
                            // Require that there WAS a separator: check raw text between espan.end and here.
                            stmts.push(Stmt::Expr {
                                id: self.fresh(),
                                expr: e,
                                span: espan,
                            });
                            let _ = stmt_start;
                        }
                    }
                }
            }
        }
    }

    /// Convert an assignment LHS expression into a place (`x`, `a.b.c`,
    /// `*r`, `**r`). Anything else is not assignable.
    fn as_place(e: Expr) -> Option<Place> {
        match e.kind {
            ExprKind::Var(name) => Some(Place::Var(name, e.span)),
            ExprKind::Field {
                base,
                field,
                field_span,
            } => {
                let inner = Self::as_place(*base)?;
                Some(Place::Field {
                    base: Box::new(inner),
                    field,
                    span: field_span,
                })
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => {
                let inner = Self::as_place(*expr)?;
                Some(Place::Deref {
                    inner: Box::new(inner),
                    span: e.span,
                })
            }
            ExprKind::Index { base, idx } => {
                let inner = Self::as_place(*base)?;
                Some(Place::Index {
                    base: Box::new(inner),
                    idx,
                    span: e.span,
                })
            }
            _ => None,
        }
    }

    fn parse_stmt(&mut self) -> Option<ParsedStmt> {
        self.skip_nls();
        match self.cur().clone() {
            Tok::Val | Tok::Var => {
                let is_val = matches!(self.cur(), Tok::Val);
                let start = self.cur_span().start;
                self.bump();
                let (name, nspan) = self.expect_ident("variable name")?;
                let mut ty = None;
                if matches!(self.cur(), Tok::Colon) {
                    self.bump();
                    ty = Some(self.parse_type()?);
                }
                // Optional `@unit` annotation (units of measure).
                let mut unit = None;
                if matches!(self.cur(), Tok::At) {
                    self.bump();
                    unit = Some(self.parse_unit_expr()?);
                }
                self.expect(Tok::Eq, "expected `=` in variable declaration")?;
                let init = self.parse_expr()?;
                let end = init.span.end;
                self.expect_separator("variable declaration")?;
                let span = Span::new(start, end);
                if is_val {
                    Some(ParsedStmt::Stmt(Stmt::Val {
                        id: self.fresh(),
                        name,
                        name_span: nspan,
                        ty,
                        unit,
                        init,
                        span,
                    }))
                } else {
                    Some(ParsedStmt::Stmt(Stmt::Var {
                        id: self.fresh(),
                        name,
                        name_span: nspan,
                        ty,
                        unit,
                        init,
                        span,
                    }))
                }
            }
            Tok::If => {
                let start = self.cur_span().start;
                self.bump();
                // `if let PAT = expr { } [else { }]` desugars to match with a
                // wildcard fallback (always exhaustive).
                if matches!(self.cur(), Tok::Let) {
                    self.bump();
                    let pat = self.parse_pat()?;
                    self.expect(Tok::Eq, "expected `=` after `if let` pattern")?;
                    let scrutinee = self.parse_nostruct_expr()?;
                    let then_b = Box::new(self.parse_block()?);
                    let else_b = {
                        let save = self.pos;
                        self.skip_nls();
                        if matches!(self.cur(), Tok::Else) {
                            self.bump();
                            Some(Box::new(self.parse_block()?))
                        } else {
                            self.pos = save;
                            None
                        }
                    };
                    let end = else_b
                        .as_ref()
                        .map(|b| b.span.end)
                        .unwrap_or(then_b.span.end);
                    let span = Span::new(start, end);
                    let fallback = match else_b {
                        Some(b) => b,
                        None => Box::new(Block {
                            id: self.fresh(),
                            stmts: vec![],
                            tail: None,
                            span: Span::new(end, end),
                        }),
                    };
                    let wild = MatchArm {
                        pat: Pat::Wild,
                        guard: None,
                        body: *fallback,
                        span: Span::new(end, end),
                    };
                    let main = MatchArm {
                        pat,
                        guard: None,
                        body: *then_b,
                        span,
                    };
                    // Statement position: a statement match (Unit arms, no
                    // result temp) — not a discarded value match.
                    self.expect_separator_opt();
                    return Some(ParsedStmt::Stmt(Stmt::Match {
                        id: self.fresh(),
                        scrutinee,
                        arms: vec![main, wild],
                        span,
                    }));
                }
                let cond = self.parse_nostruct_expr()?;
                let then_b = Box::new(self.parse_block()?);
                let mut else_b = None;
                // `else` may be on next line: skip NLs already handled by parse_block? check:
                let save = self.pos;
                self.skip_nls();
                if matches!(self.cur(), Tok::Else) {
                    self.bump();
                    if matches!(self.cur(), Tok::If) {
                        // else-if: wrap in block
                        let estip = self.parse_stmt_if_rest()?;
                        let espan = estip.span;
                        else_b = Some(Box::new(Block {
                            id: self.fresh(),
                            stmts: vec![],
                            tail: Some(Box::new(estip)),
                            span: espan,
                        }));
                    } else {
                        else_b = Some(Box::new(self.parse_block()?));
                    }
                } else {
                    self.pos = save; // NLs belong to separator logic; harmless either way
                }
                let end = else_b
                    .as_ref()
                    .map(|b| b.span.end)
                    .unwrap_or(then_b.span.end);
                let span = Span::new(start, end);
                // Trailing `if`/`else` before `}`/EOF is a tail *value* (D5 tail rule),
                // not a Unit statement — but only when `else` is present (a bare `if`
                // without `else` stays a statement so Unit functions keep working).
                if else_b.is_some() {
                    let mut p = self.pos;
                    while matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Nl)) {
                        p += 1;
                    }
                    if matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Semi)) {
                        p += 1;
                        while matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Nl)) {
                            p += 1;
                        }
                    }
                    let follows_close =
                        matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::RBrace) | None);
                    if follows_close {
                        return Some(ParsedStmt::Bare(
                            Expr {
                                id: self.fresh(),
                                span,
                                kind: ExprKind::If {
                                    cond: Box::new(cond),
                                    then_b,
                                    else_b,
                                },
                            },
                            span,
                        ));
                    }
                }
                self.expect_separator_opt();
                Some(ParsedStmt::Stmt(Stmt::If {
                    id: self.fresh(),
                    cond,
                    then_b,
                    else_b,
                    span,
                }))
            }
            Tok::Match => {
                let start = self.cur_span().start;
                self.bump();
                let (scrutinee, arms) = self.parse_match()?;
                let end = self.prev_span().end;
                let span = Span::new(start, end);
                // Tail-position match is a value (D5 tail rule).
                let mut q = self.pos;
                while matches!(self.toks.get(q).map(|t| &t.kind), Some(Tok::Nl)) {
                    q += 1;
                }
                if matches!(self.toks.get(q).map(|t| &t.kind), Some(Tok::Semi)) {
                    q += 1;
                    while matches!(self.toks.get(q).map(|t| &t.kind), Some(Tok::Nl)) {
                        q += 1;
                    }
                }
                let follows_close =
                    matches!(self.toks.get(q).map(|t| &t.kind), Some(Tok::RBrace) | None);
                if follows_close {
                    return Some(ParsedStmt::Bare(
                        Expr {
                            id: self.fresh(),
                            span,
                            kind: ExprKind::Match {
                                scrutinee: Box::new(scrutinee),
                                arms,
                            },
                        },
                        span,
                    ));
                }
                self.expect_separator_opt();
                Some(ParsedStmt::Stmt(Stmt::Match {
                    id: self.fresh(),
                    scrutinee,
                    arms,
                    span,
                }))
            }
            Tok::While => {
                let start = self.cur_span().start;
                self.bump();
                let cond = self.parse_nostruct_expr()?;
                let body = Box::new(self.parse_block()?);
                let end = body.span.end;
                self.expect_separator_opt();
                Some(ParsedStmt::Stmt(Stmt::While {
                    id: self.fresh(),
                    cond,
                    body,
                    span: Span::new(start, end),
                }))
            }
            Tok::For => {
                let start = self.cur_span().start;
                self.bump();
                self.expect(Tok::LParen, "expected `(` after `for`")?;
                self.skip_nls();
                let (var, var_span) = self.expect_ident("loop variable")?;
                match self.cur() {
                    Tok::In => self.bump(),
                    _ => {
                        self.err("E0005", "expected `in` after loop variable");
                        return None;
                    }
                }
                let head = self.parse_nostruct_expr()?;
                let iter = match self.cur() {
                    Tok::DotDot | Tok::DotDotEq => {
                        let inclusive = matches!(self.cur(), Tok::DotDotEq);
                        self.bump();
                        let hi = self.parse_nostruct_expr()?;
                        ForIter::Range {
                            lo: head,
                            hi,
                            inclusive,
                        }
                    }
                    // No `..`: iterate a `Vec` value (sema validates).
                    _ => ForIter::Each { vec: head },
                };
                self.skip_nls();
                self.expect(Tok::RParen, "expected `)` after `for` header")?;
                let body = Box::new(self.parse_block()?);
                let end = body.span.end;
                self.expect_separator_opt();
                Some(ParsedStmt::Stmt(Stmt::For {
                    id: self.fresh(),
                    var,
                    var_span,
                    iter,
                    body,
                    span: Span::new(start, end),
                }))
            }
            Tok::Loop => {
                let start = self.cur_span().start;
                self.bump();
                let body = Box::new(self.parse_block()?);
                let end = body.span.end;
                let span = Span::new(start, end);
                // Trailing `loop` before `}`/EOF is a tail *value* (may carry
                // `break v`), mirroring the `if`/`else` tail rule.
                let mut p = self.pos;
                while matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Nl)) {
                    p += 1;
                }
                if matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Semi)) {
                    p += 1;
                    while matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Nl)) {
                        p += 1;
                    }
                }
                let follows_close =
                    matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::RBrace) | None);
                if follows_close {
                    return Some(ParsedStmt::Bare(
                        Expr {
                            id: self.fresh(),
                            span,
                            kind: ExprKind::Loop { body },
                        },
                        span,
                    ));
                }
                self.expect_separator_opt();
                Some(ParsedStmt::Stmt(Stmt::Loop {
                    id: self.fresh(),
                    body,
                    span,
                }))
            }
            Tok::Break => {
                let start = self.cur_span().start;
                self.bump();
                self.skip_nls();
                let value = match self.cur() {
                    Tok::Semi | Tok::RBrace | Tok::Eof | Tok::Nl => None,
                    _ => Some(self.parse_expr()?),
                };
                self.expect_separator("break statement")?;
                let end = value.as_ref().map(|e| e.span.end).unwrap_or(start + 5);
                Some(ParsedStmt::Stmt(Stmt::Break {
                    id: self.fresh(),
                    value,
                    span: Span::new(start, end),
                }))
            }
            Tok::Continue => {
                let start = self.cur_span().start;
                self.bump();
                self.expect_separator("continue statement")?;
                Some(ParsedStmt::Stmt(Stmt::Continue {
                    id: self.fresh(),
                    span: Span::new(start, start + 8),
                }))
            }
            Tok::Return => {
                let start = self.cur_span().start;
                self.bump();
                // `return` with value unless separator/`}` follows
                self.skip_nls();
                let value = match self.cur() {
                    Tok::Semi | Tok::RBrace | Tok::Eof => None,
                    _ => Some(self.parse_expr()?),
                };
                self.expect_separator("return statement")?;
                let end = value.as_ref().map(|e| e.span.end).unwrap_or(start + 6);
                Some(ParsedStmt::Stmt(Stmt::Return {
                    id: self.fresh(),
                    value,
                    span: Span::new(start, end),
                }))
            }
            Tok::Semi => {
                self.bump();
                self.parse_stmt()
            }
            _ => {
                // bare expression (maybe assignment)
                let e = self.parse_expr()?;
                let espan = e.span;
                // assignment?
                match self.cur() {
                    Tok::Eq
                    | Tok::PlusEq
                    | Tok::MinusEq
                    | Tok::StarEq
                    | Tok::SlashEq
                    | Tok::PercentEq => {
                        let op = match self.cur() {
                            Tok::Eq => AssignOp::Set,
                            Tok::PlusEq => AssignOp::Add,
                            Tok::MinusEq => AssignOp::Sub,
                            Tok::StarEq => AssignOp::Mul,
                            Tok::SlashEq => AssignOp::Div,
                            _ => AssignOp::Rem,
                        };
                        let aspan = self.cur_span();
                        self.bump();
                        let value = self.parse_expr()?;
                        let end = value.span.end;
                        self.expect_separator("assignment")?;
                        match Self::as_place(e) {
                            Some(target) => {
                                let ae = Expr {
                                    id: self.fresh(),
                                    span: Span::new(espan.start, end),
                                    kind: ExprKind::Assign {
                                        target,
                                        op,
                                        value: Box::new(value),
                                    },
                                };
                                let _ = aspan;
                                // assignments are statements; wrap as Expr stmt w/ separator consumed
                                Some(ParsedStmt::Stmt(Stmt::Expr {
                                    id: self.fresh(),
                                    expr: ae,
                                    span: Span::new(espan.start, end),
                                }))
                            }
                            None => {
                                self.err_at("E0005", espan, "assignment target must be a variable, field path, or dereference");
                                None
                            }
                        }
                    }
                    _ => Some(ParsedStmt::Bare(e, espan)),
                }
            }
        }
    }

    fn parse_stmt_if_rest(&mut self) -> Option<Expr> {
        // after `else` when next is `if`: parse if-expression
        debug_assert!(matches!(self.cur(), Tok::If));
        self.bump();
        let start_span = self.cur_span();
        let cond = self.parse_nostruct_expr()?;
        let then_b = Box::new(self.parse_block()?);
        let mut else_b = None;
        let save = self.pos;
        self.skip_nls();
        if matches!(self.cur(), Tok::Else) {
            self.bump();
            if matches!(self.cur(), Tok::If) {
                let e = self.parse_stmt_if_rest()?;
                let sp = e.span;
                else_b = Some(Box::new(Block {
                    id: self.fresh(),
                    stmts: vec![],
                    tail: Some(Box::new(e)),
                    span: sp,
                }));
            } else {
                else_b = Some(Box::new(self.parse_block()?));
            }
        } else {
            self.pos = save;
        }
        let end = else_b
            .as_ref()
            .map(|b| b.span.end)
            .unwrap_or(then_b.span.end);
        Some(Expr {
            id: self.fresh(),
            span: Span::new(start_span.start, end),
            kind: ExprKind::If {
                cond: Box::new(cond),
                then_b,
                else_b,
            },
        })
    }

    // ---------- expressions (precedence climbing) ----------

    fn parse_expr(&mut self) -> Option<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_and()?;
        loop {
            if !matches!(self.peek_next(), Tok::OrOr) {
                break;
            }
            self.skip_nls();
            self.bump();
            let rhs = self.parse_and()?;
            lhs = self.binary(BinOp::Or, lhs, rhs);
        }
        Some(lhs)
    }

    fn parse_and(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_cmp()?;
        loop {
            if !matches!(self.peek_next(), Tok::AndAnd) {
                break;
            }
            self.skip_nls();
            self.bump();
            let rhs = self.parse_cmp()?;
            lhs = self.binary(BinOp::And, lhs, rhs);
        }
        Some(lhs)
    }

    fn parse_cmp(&mut self) -> Option<Expr> {
        let lhs = self.parse_add()?;
        let op = match self.peek_next() {
            Tok::EqEq => BinOp::Eq,
            Tok::BangEq => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::LtEq => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::GtEq => BinOp::Ge,
            _ => return Some(lhs),
        };
        self.skip_nls();
        self.bump();
        let rhs = self.parse_add()?;
        // Non-associative: reject chains like a < b < c
        if matches!(
            self.peek_next(),
            Tok::EqEq | Tok::BangEq | Tok::Lt | Tok::LtEq | Tok::Gt | Tok::GtEq
        ) {
            self.err(
                "E0005",
                "comparison operators do not chain; use parentheses",
            );
            return None;
        }
        Some(self.binary(op, lhs, rhs))
    }

    fn parse_add(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek_next() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.skip_nls();
            self.bump();
            let rhs = self.parse_mul()?;
            lhs = self.binary(op, lhs, rhs);
        }
        Some(lhs)
    }

    fn parse_mul(&mut self) -> Option<Expr> {
        let mut lhs = self.parse_cast()?;
        loop {
            let op = match self.peek_next() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Rem,
                _ => break,
            };
            self.skip_nls();
            self.bump();
            let rhs = self.parse_cast()?;
            lhs = self.binary(op, lhs, rhs);
        }
        Some(lhs)
    }

    /// `as` conversions bind tighter than `*` (D5 precedence).
    /// `as @unit` asserts/converts dimension, keeping the operand type.
    fn parse_cast(&mut self) -> Option<Expr> {
        let mut expr = self.parse_unary()?;
        loop {
            if !matches!(self.peek_next(), Tok::As) {
                break;
            }
            self.skip_nls();
            self.bump(); // as
            if matches!(self.cur(), Tok::At) {
                self.bump();
                let unit = self.parse_unit_expr()?;
                let end = self.prev_span().end;
                let start = expr.span.start;
                expr = Expr {
                    id: self.fresh(),
                    span: Span::new(start, end),
                    kind: ExprKind::AsUnit {
                        expr: Box::new(expr),
                        unit: Box::new(unit),
                    },
                };
                continue;
            }
            let target_span = self.cur_span();
            let target = self.parse_type()?;
            let end = target_span.end;
            let start = expr.span.start;
            expr = Expr {
                id: self.fresh(),
                span: Span::new(start, end),
                kind: ExprKind::Cast {
                    expr: Box::new(expr),
                    target,
                    target_span,
                },
            };
        }
        Some(expr)
    }

    fn parse_unary(&mut self) -> Option<Expr> {
        self.skip_nls();
        match self.cur() {
            Tok::Minus => {
                let s = self.cur_span().start;
                self.bump();
                let e = self.parse_unary()?;
                let end = e.span.end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(s, end),
                    kind: ExprKind::Unary {
                        op: UnOp::Neg,
                        expr: Box::new(e),
                    },
                })
            }
            Tok::Bang => {
                let s = self.cur_span().start;
                self.bump();
                let e = self.parse_unary()?;
                let end = e.span.end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(s, end),
                    kind: ExprKind::Unary {
                        op: UnOp::Not,
                        expr: Box::new(e),
                    },
                })
            }
            Tok::Amp | Tok::AmpMut => {
                let mutable = matches!(self.cur(), Tok::AmpMut);
                let s = self.cur_span().start;
                self.bump();
                let mut mutable = mutable;
                if !mutable && matches!(self.cur(), Tok::Mut) {
                    self.bump();
                    mutable = true;
                }
                let e = self.parse_unary()?;
                let end = e.span.end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(s, end),
                    kind: ExprKind::AddrOf {
                        expr: Box::new(e),
                        mutable,
                    },
                })
            }
            Tok::Star => {
                let s = self.cur_span().start;
                self.bump();
                let e = self.parse_unary()?;
                let end = e.span.end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(s, end),
                    kind: ExprKind::Unary {
                        op: UnOp::Deref,
                        expr: Box::new(e),
                    },
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Option<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            match self.cur() {
                Tok::LParen => {
                    // call: only plain `name(args)` in 0.1.0
                    match &e.kind {
                        ExprKind::Var(name) => {
                            let name = name.clone();
                            self.bump();
                            let mut args = vec![];
                            loop {
                                self.skip_nls();
                                if matches!(self.cur(), Tok::RParen) {
                                    self.bump();
                                    break;
                                }
                                let a = self.parse_expr()?;
                                args.push(a);
                                self.skip_nls();
                                match self.cur() {
                                    Tok::Comma => {
                                        self.bump();
                                    }
                                    Tok::RParen => {}
                                    _ => {
                                        self.err("E0005", "expected `,` or `)` in call arguments");
                                        return None;
                                    }
                                }
                            }
                            let end = self.prev_span().end;
                            e = Expr {
                                id: self.fresh(),
                                span: Span::new(e.span.start, end),
                                kind: ExprKind::Call { name, args },
                            };
                        }
                        _ => {
                            self.err(
                                "E0005",
                                "only direct function calls are supported in Valvet 0.1.0",
                            );
                            return None;
                        }
                    }
                }
                Tok::Question => {
                    self.bump();
                    let end = self.prev_span().end;
                    e = Expr {
                        id: self.fresh(),
                        span: Span::new(e.span.start, end),
                        kind: ExprKind::Try { expr: Box::new(e) },
                    };
                }
                Tok::DotQuestion => {
                    // `base?.field` (methods via `?.(` stay E0800 until Phase F).
                    self.bump();
                    match self.cur().clone() {
                        Tok::Ident(field) => {
                            let fspan = self.cur_span();
                            self.bump();
                            if matches!(self.cur(), Tok::LParen) {
                                self.err(
                                    "E0800",
                                    "method calls are not yet supported in Valvet 0.1.0",
                                );
                                return None;
                            }
                            let end = fspan.end;
                            e = Expr {
                                id: self.fresh(),
                                span: Span::new(e.span.start, end),
                                kind: ExprKind::OptField {
                                    base: Box::new(e),
                                    field,
                                    field_span: fspan,
                                },
                            };
                        }
                        _ => {
                            self.err("E0005", "expected field name after `?.`");
                            return None;
                        }
                    }
                }
                Tok::Dot => {
                    // `.name` / `.0` = struct field access;
                    // `.name(` = method call (Phase F).
                    self.bump();
                    let (field, fspan) = match self.cur().clone() {
                        Tok::Ident(field) => {
                            let fspan = self.cur_span();
                            self.bump();
                            (field, fspan)
                        }
                        // Tuple field access `pair.0` (lexer produces IntLit).
                        Tok::IntLit(n) => {
                            let fspan = self.cur_span();
                            self.bump();
                            (format!("_{}", n), fspan)
                        }
                        _ => {
                            self.err("E0005", "expected field name after `.`");
                            return None;
                        }
                    };
                    if matches!(self.cur(), Tok::LParen) {
                        // `recv.method(args)` — 0.1.0 resolves `Vec` methods in
                        // sema; anything else is E0401 there.
                        self.bump();
                        let mut args = vec![];
                        loop {
                            self.skip_nls();
                            if matches!(self.cur(), Tok::RParen) {
                                self.bump();
                                break;
                            }
                            args.push(self.parse_expr()?);
                            self.skip_nls();
                            match self.cur() {
                                Tok::Comma => {
                                    self.bump();
                                }
                                Tok::RParen => {}
                                _ => {
                                    self.err("E0005", "expected `,` or `)` in call arguments");
                                    return None;
                                }
                            }
                        }
                        let end = self.prev_span().end;
                        e = Expr {
                            id: self.fresh(),
                            span: Span::new(e.span.start, end),
                            kind: ExprKind::Method {
                                receiver: Box::new(e),
                                method: field,
                                method_span: fspan,
                                args,
                            },
                        };
                        // Postfix loop continues (chained `.a.b()`)
                        continue;
                    }
                    let end = fspan.end;
                    e = Expr {
                        id: self.fresh(),
                        span: Span::new(e.span.start, end),
                        kind: ExprKind::Field {
                            base: Box::new(e),
                            field,
                            field_span: fspan,
                        },
                    };
                }
                Tok::LBracket => {
                    // `base[idx]` element access (J1).
                    self.bump();
                    self.skip_nls();
                    let idx = self.parse_expr()?;
                    self.skip_nls();
                    if !matches!(self.cur(), Tok::RBracket) {
                        self.err("E0005", "expected `]` after index expression");
                        return None;
                    }
                    self.bump();
                    let end = self.prev_span().end;
                    e = Expr {
                        id: self.fresh(),
                        span: Span::new(e.span.start, end),
                        kind: ExprKind::Index {
                            base: Box::new(e),
                            idx: Box::new(idx),
                        },
                    };
                }
                _ => break,
            }
        }
        Some(e)
    }

    fn parse_primary(&mut self) -> Option<Expr> {
        self.skip_nls();
        let sp = self.cur_span();
        match self.cur().clone() {
            Tok::IntLit(v) => {
                self.bump();
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Int(v),
                })
            }
            Tok::FloatLit(v) => {
                self.bump();
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Float(v),
                })
            }
            Tok::True => {
                self.bump();
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Bool(true),
                })
            }
            Tok::False => {
                self.bump();
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Bool(false),
                })
            }
            Tok::StrLit(s) => {
                self.bump();
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Str(s),
                })
            }
            Tok::CharLit(_) => {
                self.err(
                    "E0800",
                    "character literals are not yet supported in Valvet 0.1.0",
                );
                None
            }
            Tok::Ident(name) => {
                // `vec![...]` — the sole builtin macro (homogeneous elements).
                if name == "vec" {
                    let save = self.pos;
                    self.bump();
                    if matches!(self.cur(), Tok::Bang) {
                        self.bump();
                        self.expect(Tok::LBracket, "expected `[` after `vec!`")?;
                        let mut elems = vec![];
                        loop {
                            self.skip_nls();
                            if matches!(self.cur(), Tok::RBracket) {
                                self.bump();
                                break;
                            }
                            elems.push(self.parse_expr()?);
                            self.skip_nls();
                            match self.cur() {
                                Tok::Comma => {
                                    self.bump();
                                }
                                Tok::RBracket => {}
                                _ => {
                                    self.err("E0005", "expected `,` or `]` in `vec![...]`");
                                    return None;
                                }
                            }
                        }
                        let end = self.prev_span().end;
                        return Some(Expr {
                            id: self.fresh(),
                            span: Span::new(sp.start, end),
                            kind: ExprKind::VecMacro { elems },
                        });
                    }
                    self.pos = save;
                }
                self.bump();
                // `Type::Variant(args)` — enum variant construction.
                if matches!(self.cur(), Tok::ColonColon) {
                    self.bump();
                    let (vname, vspan) = self.expect_ident("variant name")?;
                    let mut args = vec![];
                    if matches!(self.cur(), Tok::LParen) {
                        self.bump();
                        loop {
                            self.skip_nls();
                            if matches!(self.cur(), Tok::RParen) {
                                self.bump();
                                break;
                            }
                            args.push(self.parse_expr()?);
                            self.skip_nls();
                            match self.cur() {
                                Tok::Comma => {
                                    self.bump();
                                }
                                Tok::RParen => {}
                                _ => {
                                    self.err("E0005", "expected `,` or `)` in variant arguments");
                                    return None;
                                }
                            }
                        }
                    }
                    let end = self.prev_span().end;
                    return Some(Expr {
                        id: self.fresh(),
                        span: Span::new(sp.start, end),
                        kind: ExprKind::Variant {
                            ty: name,
                            ty_span: sp,
                            variant: vname,
                            var_span: vspan,
                            args,
                        },
                    });
                }
                // `Name { field: value, ... }` — struct literal (expression
                // position only; forbidden where `{` opens a block).
                if self.no_struct == 0 && matches!(self.cur(), Tok::LBrace) {
                    self.bump();
                    let mut fields = vec![];
                    loop {
                        self.skip_nls();
                        if matches!(self.cur(), Tok::RBrace) {
                            self.bump();
                            break;
                        }
                        let (fname, fspan) = self.expect_ident("field name")?;
                        self.expect(Tok::Colon, "expected `:` after field name")?;
                        let fexpr = self.parse_expr()?;
                        fields.push((fname, fspan, fexpr));
                        self.skip_nls();
                        match self.cur() {
                            Tok::Comma => {
                                self.bump();
                            }
                            Tok::RBrace => {}
                            _ => {
                                self.err("E0005", "expected `,` or `}` in struct literal");
                                return None;
                            }
                        }
                    }
                    let end = self.prev_span().end;
                    return Some(Expr {
                        id: self.fresh(),
                        span: Span::new(sp.start, end),
                        kind: ExprKind::StructLit {
                            name,
                            name_span: sp,
                            fields,
                        },
                    });
                }
                Some(Expr {
                    id: self.fresh(),
                    span: sp,
                    kind: ExprKind::Var(name),
                })
            }
            Tok::LParen => {
                self.bump();
                self.skip_nls();
                // Restore-before-`?`: the flag must survive inner failures.
                let saved_struct = self.no_struct;
                self.no_struct = 0;
                let inner = self.parse_paren_inner(sp);
                self.no_struct = saved_struct;
                inner
            }
            Tok::If => {
                self.bump();
                let cond = self.parse_nostruct_expr()?;
                let then_b = Box::new(self.parse_block()?);
                let mut else_b = None;
                let save = self.pos;
                self.skip_nls();
                if matches!(self.cur(), Tok::Else) {
                    self.bump();
                    if matches!(self.cur(), Tok::If) {
                        let e = self.parse_stmt_if_rest()?;
                        let esp = e.span;
                        else_b = Some(Box::new(Block {
                            id: self.fresh(),
                            stmts: vec![],
                            tail: Some(Box::new(e)),
                            span: esp,
                        }));
                    } else {
                        else_b = Some(Box::new(self.parse_block()?));
                    }
                } else {
                    self.pos = save;
                }
                let end = else_b
                    .as_ref()
                    .map(|b| b.span.end)
                    .unwrap_or(then_b.span.end);
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(sp.start, end),
                    kind: ExprKind::If {
                        cond: Box::new(cond),
                        then_b,
                        else_b,
                    },
                })
            }
            Tok::Match => {
                self.bump();
                let (scrutinee, arms) = self.parse_match()?;
                let end = self.prev_span().end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(sp.start, end),
                    kind: ExprKind::Match {
                        scrutinee: Box::new(scrutinee),
                        arms,
                    },
                })
            }
            Tok::LBrace => {
                self.err(
                    "E0005",
                    "unexpected block; blocks are not expressions in statement position here",
                );
                None
            }
            Tok::Loop => {
                self.bump();
                let body = Box::new(self.parse_block()?);
                let end = body.span.end;
                Some(Expr {
                    id: self.fresh(),
                    span: Span::new(sp.start, end),
                    kind: ExprKind::Loop { body },
                })
            }
            t => {
                self.err(
                    "E0005",
                    format!("expected expression, found {}", describe(&t)),
                );
                None
            }
        }
    }

    // ---------- helpers ----------

    fn binary(&mut self, op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        let start = lhs.span.start;
        let end = rhs.span.end;
        Expr {
            id: self.fresh(),
            span: Span::new(start, end),
            kind: ExprKind::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        }
    }

    fn fresh(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn cur(&self) -> &Tok {
        match self.toks.get(self.pos) {
            Some(t) => &t.kind,
            None => &Tok::Eof,
        }
    }

    fn cur_span(&self) -> Span {
        self.toks
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::empty(usize::MAX / 2))
    }

    fn prev_span(&self) -> Span {
        self.toks
            .get(self.pos.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(Span::empty(0))
    }

    fn at_ident(&self, w: &str) -> bool {
        matches!(self.cur(), Tok::Ident(n) if n == w)
    }

    /// True when the current token can start an item (mirrors `parse_item`'s
    /// dispatch). The top-level loop only attempts script-statement parsing
    /// when this is false, so broken items keep their own diagnostics.
    fn at_item_start(&self) -> bool {
        match self.cur() {
            Tok::Fun
            | Tok::Struct
            | Tok::Enum
            | Tok::Pub
            | Tok::Unsafe
            | Tok::Extern
            | Tok::Trait
            | Tok::Impl
            | Tok::Type
            | Tok::Const
            | Tok::Static
            | Tok::Defer
            | Tok::Use
            | Tok::Where
            | Tok::LBracket => true,
            Tok::Ident(w) if w == "import" || w == "module" => true,
            _ => false,
        }
    }

    fn bump(&mut self) {
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
    }

    fn skip_nls(&mut self) {
        while matches!(self.cur(), Tok::Nl) {
            self.pos += 1;
        }
    }

    /// Parse an expression that may not start a struct literal (condition /
    /// scrutinee positions where `{` opens a block). Exception-safe: the flag
    /// is always restored, even when parsing fails partway (recovery relies
    /// on it for subsequent items).
    fn parse_nostruct_expr(&mut self) -> Option<Expr> {
        self.no_struct += 1;
        let r = self.parse_expr();
        self.no_struct -= 1;
        r
    }

    /// Next non-newline token without consuming input (newline-sensitive lookahead).
    /// At bracket depth 0 newlines terminate expressions: a binary operator
    /// starting the next line begins a new statement (`*p` is dereference,
    /// not multiplication). Inside `()`/`[]` newlines are insignificant.
    fn peek_next(&self) -> Tok {
        if self.bracket_depth() == 0 {
            return self.cur().clone();
        }
        let mut p = self.pos;
        while matches!(self.toks.get(p).map(|t| &t.kind), Some(Tok::Nl)) {
            p += 1;
        }
        self.toks.get(p).map(|t| t.kind.clone()).unwrap_or(Tok::Eof)
    }

    /// Round/square bracket depth at the current position, recomputed from
    /// the token stream (robust to speculative save/restore).
    fn bracket_depth(&self) -> i32 {
        let mut d: i32 = 0;
        for t in self.toks.iter().take(self.pos) {
            match &t.kind {
                Tok::LParen | Tok::LBracket => d += 1,
                Tok::RParen | Tok::RBracket => d -= 1,
                _ => {}
            }
        }
        d
    }

    fn skip_balanced(&mut self, open: Tok, close: Tok) {
        let mut depth = 0;
        loop {
            if self.cur() == &open {
                depth += 1;
            } else if self.cur() == &close {
                if depth == 0 {
                    self.bump();
                    return;
                }
                depth -= 1;
            } else if matches!(self.cur(), Tok::Eof) {
                return;
            }
            self.pos += 1;
        }
    }

    fn expect(&mut self, t: Tok, msg: &str) -> Option<()> {
        if self.cur() == &t {
            self.bump();
            Some(())
        } else {
            self.err("E0005", msg);
            None
        }
    }

    fn expect_ident(&mut self, what: &str) -> Option<(String, Span)> {
        match self.cur().clone() {
            Tok::Ident(n) => {
                let s = self.cur_span();
                self.bump();
                Some((n, s))
            }
            _ => {
                self.err(
                    "E0005",
                    format!("expected {}, found {}", what, describe(self.cur())),
                );
                None
            }
        }
    }

    fn expect_semi(&mut self, what: &str) -> Option<()> {
        match self.cur() {
            Tok::Semi => {
                self.bump();
                Some(())
            }
            Tok::Nl => {
                self.bump();
                Some(())
            }
            _ => {
                self.err("E0004", format!("expected `;` or newline after {}", what));
                None
            }
        }
    }

    fn expect_separator(&mut self, what: &str) -> Option<()> {
        match self.cur() {
            Tok::Semi => {
                self.bump();
                self.skip_nls();
                Some(())
            }
            Tok::Nl => {
                self.skip_nls();
                Some(())
            }
            Tok::RBrace | Tok::Eof => Some(()),
            _ => {
                self.err("E0004", format!("expected `;` or newline after {}", what));
                None
            }
        }
    }

    fn expect_separator_opt(&mut self) {
        match self.cur() {
            Tok::Semi => {
                self.bump();
                self.skip_nls();
            }
            Tok::Nl => self.skip_nls(),
            _ => {}
        }
    }

    fn err(&mut self, code: &'static str, msg: impl Into<String>) {
        let sp = self.cur_span();
        self.err_at(code, sp, msg);
    }

    fn err_at(&mut self, code: &'static str, sp: Span, msg: impl Into<String>) {
        // Deduplicate identical consecutive errors to avoid cascades.
        let m = msg.into();
        if let Some(last) = self.errs.last() {
            if last.code == code && last.message == m && last.span == sp {
                return;
            }
        }
        self.errs.push(Diagnostic::error(code, sp, m));
    }

    /// Error recovery: skip to next likely item boundary.
    fn recover(&mut self) {
        let mut depth = 0;
        loop {
            match self.cur() {
                Tok::Eof => return,
                Tok::LBrace => {
                    depth += 1;
                    self.bump();
                }
                Tok::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    self.bump();
                }
                Tok::Fun if depth == 0 => return,
                Tok::Nl | Tok::Semi if depth == 0 => {
                    self.bump();
                    // after a separator at depth 0, if next is `fun` or EOF we're synced;
                    // otherwise keep skipping one more token? Return and let parse_item decide.
                    if matches!(self.cur(), Tok::Fun | Tok::Eof) {
                        return;
                    }
                    // If next looks like start of fun (`pub`), sync too.
                    if matches!(self.cur(), Tok::Pub) {
                        return;
                    }
                    return;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }
}

enum ParsedStmt {
    Stmt(Stmt),
    Bare(Expr, Span),
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Eof => "end of file".into(),
        Tok::Nl => "newline".into(),
        Tok::Ident(n) => format!("identifier `{}`", n),
        Tok::IntLit(_) => "integer literal".into(),
        Tok::FloatLit(_) => "float literal".into(),
        Tok::StrLit(_) => "string literal".into(),
        Tok::CharLit(_) => "character literal".into(),
        other => format!("`{:?}`", other),
    }
}
