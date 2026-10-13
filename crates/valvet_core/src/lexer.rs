use crate::diag::Diagnostic;
use crate::source::{Source, Span};
use crate::token::{keyword, Tok, Token};

/// Handwritten lexer. Never panics on user input; reports E0001/E0003/E0010.
pub fn lex(source: &Source) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        text: source.text.as_bytes(),
        pos: 0,
        out: vec![],
        errs: vec![],
    };
    lx.run();
    (lx.out, lx.errs)
}

struct Lexer<'a> {
    text: &'a [u8],
    pos: usize,
    out: Vec<Token>,
    errs: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn run(&mut self) {
        // Script shebang: a leading `#!...` line is skipped so Valvet
        // files can be direct executables (`chmod +x`).
        let mut i = 0;
        if self.text.len() >= 2 && self.text[0] == b'#' && self.text[1] == b'!' {
            i = 2;
            while i < self.text.len() && self.text[i] != b'\n' {
                i += 1;
            }
        }
        // UTF-8 validity is guaranteed by Rust String; validate scalars below.
        while i < self.text.len() {
            let c = self.text[i];
            match c {
                b' ' | b'\t' | b'\r' => {
                    // CRLF normalised: lone \r skipped; \r\n handled by \n arm.
                    i += 1;
                }
                b'\n' => {
                    self.push(Tok::Nl, i, i + 1);
                    i += 1;
                }
                b'/' if self.peek(i + 1) == Some(b'/') => {
                    i += 2;
                    while i < self.text.len() && self.text[i] != b'\n' {
                        i += 1;
                    }
                }
                b'/' if self.peek(i + 1) == Some(b'*') => {
                    let start = i;
                    i += 2;
                    let mut depth = 1usize;
                    while i < self.text.len() && depth > 0 {
                        if self.text[i] == b'/' && self.peek(i + 1) == Some(b'*') {
                            depth += 1;
                            i += 2;
                        } else if self.text[i] == b'*' && self.peek(i + 1) == Some(b'/') {
                            depth -= 1;
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    if depth > 0 {
                        self.errs.push(
                            Diagnostic::error(
                                "E0003",
                                Span::new(start, self.text.len()),
                                "unterminated block comment",
                            )
                            .with_help("close the comment with */"),
                        );
                    }
                }
                b'"' => i = self.string(i),
                b'\'' => i = self.char_lit(i),
                b'0'..=b'9' => i = self.number(i),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => i = self.word(i),
                _ => {
                    i = self.symbol(i);
                }
            }
        }
        let e = self.text.len();
        self.out.push(Token {
            kind: Tok::Eof,
            span: Span::new(e, e),
        });
        self.pos = i;
    }

    fn peek(&self, i: usize) -> Option<u8> {
        self.text.get(i).copied()
    }

    fn push(&mut self, kind: Tok, s: usize, e: usize) {
        self.out.push(Token {
            kind,
            span: Span::new(s, e),
        });
    }

    fn word(&mut self, mut i: usize) -> usize {
        let s = i;
        // Raw identifier r#kw
        if self.text[i] == b'r' && self.peek(i + 1) == Some(b'#') {
            i += 2;
            let ns = i;
            while i < self.text.len() && is_ident(self.text[i]) {
                i += 1;
            }
            let name = String::from_utf8_lossy(&self.text[ns..i]).into_owned();
            if name.is_empty() {
                self.errs.push(Diagnostic::error(
                    "E0010",
                    Span::new(s, i),
                    "expected identifier after r#",
                ));
                return i;
            }
            self.push(Tok::Ident(name), s, i);
            return i;
        }
        while i < self.text.len() && is_ident(self.text[i]) {
            i += 1;
        }
        let w = String::from_utf8_lossy(&self.text[s..i]).into_owned();
        match keyword(&w) {
            Some(k) => self.push(k, s, i),
            None => self.push(Tok::Ident(w), s, i),
        }
        i
    }

    fn number(&mut self, s: usize) -> usize {
        let mut i = s;
        let mut base = 10;
        if self.text[i] == b'0' && i + 1 < self.text.len() {
            match self.text[i + 1] {
                b'x' => base = 16,
                b'o' => base = 8,
                b'b' => base = 2,
                _ => {}
            }
            if base != 10 {
                i += 2;
            }
        }
        let ds = i;
        while i < self.text.len() && is_digit_base(self.text[i], base) {
            i += 1;
        }
        // Digit separators (not leading/trailing/double — enforced loosely: reject empty runs)
        let mut digits = String::new();
        let mut j = ds;
        let mut last_us = true;
        while j < i {
            let c = self.text[j];
            if c == b'_' {
                if last_us || j + 1 >= i || self.text[j + 1] == b'_' {
                    self.errs.push(Diagnostic::error(
                        "E0010",
                        Span::new(s, i),
                        "misplaced digit separator `_`",
                    ));
                    break;
                }
                last_us = true;
                j += 1;
                continue;
            }
            last_us = false;
            digits.push(c as char);
            j += 1;
        }
        // Float?
        let mut is_float = false;
        if base == 10 {
            if self.peek(i) == Some(b'.') && matches!(self.peek(i + 1), Some(b'0'..=b'9')) {
                is_float = true;
                digits.push('.');
                i += 1;
                while i < self.text.len() && (self.text[i].is_ascii_digit() || self.text[i] == b'_')
                {
                    if self.text[i] != b'_' {
                        digits.push(self.text[i] as char);
                    }
                    i += 1;
                }
            }
            if matches!(self.peek(i), Some(b'e') | Some(b'E')) {
                is_float = true;
                digits.push('e');
                i += 1;
                if matches!(self.peek(i), Some(b'+') | Some(b'-')) {
                    digits.push(self.text[i] as char);
                    i += 1;
                }
                let mut any = false;
                while i < self.text.len() && self.text[i].is_ascii_digit() {
                    digits.push(self.text[i] as char);
                    i += 1;
                    any = true;
                }
                if !any {
                    self.errs.push(Diagnostic::error(
                        "E0010",
                        Span::new(s, i),
                        "malformed float exponent",
                    ));
                    return i;
                }
            }
        }
        // Suffix
        let suf_s = i;
        while i < self.text.len() && (self.text[i].is_ascii_alphanumeric() || self.text[i] == b'_')
        {
            // suffixes attach only if they look like type suffixes; break on operator-ish? keep simple:
            // consume alnum run only when immediately adjacent (it is) — but `42 + x` has space so fine.
            // `2.5f32` handled here.
            i += 1;
        }
        let suffix = String::from_utf8_lossy(&self.text[suf_s..i]).into_owned();
        // Numeric suffixes (C1): only Valvet's own types survive the lexer;
        // anything else is E0010. `f64` needs a float literal, `i32`/`i64`
        // need an integer literal.
        let suffix_tok = match suffix.as_str() {
            "" => None,
            "i32" if !is_float => Some(crate::token::NumSuffix::I32),
            "i64" if !is_float => Some(crate::token::NumSuffix::I64),
            "f64" if is_float => Some(crate::token::NumSuffix::F64),
            _ => {
                self.errs.push(Diagnostic::error(
                    "E0010",
                    Span::new(s, i),
                    format!("invalid numeric suffix `{}`", suffix),
                ));
                return i;
            }
        };
        if is_float {
            match digits.parse::<f64>() {
                Ok(v) => self.push(Tok::FloatLit(v), s, suf_s),
                Err(_) => self.errs.push(Diagnostic::error(
                    "E0010",
                    Span::new(s, i),
                    "malformed float literal",
                )),
            }
        } else {
            match i64::from_str_radix(digits.as_str().max("0"), base) {
                Ok(v) => self.push(Tok::IntLit(v), s, suf_s),
                Err(_) => self.errs.push(Diagnostic::error(
                    "E0010",
                    Span::new(s, i),
                    "integer literal out of range",
                )),
            }
        }
        if let Some(k) = suffix_tok {
            self.push(Tok::IntSuffix(k), suf_s, i);
        }
        i
    }

    fn string(&mut self, s: usize) -> usize {
        // Normal "..." and multiline """...""" (raw r".." deferred: lex as ident r + string is wrong,
        // so handle r" prefix here).
        let mut i = s;
        if self.text[s] == b'r' {
            // shouldn't happen (word() handles r#); bare r"..." raw string: support simply.
        }
        if self.text[i..].starts_with(b"\"\"\"") {
            i += 3;
            let mut out = String::new();
            loop {
                if i >= self.text.len() {
                    self.errs.push(Diagnostic::error(
                        "E0003",
                        Span::new(s, i),
                        "unterminated string literal",
                    ));
                    return i;
                }
                if self.text[i..].starts_with(b"\"\"\"") {
                    i += 3;
                    break;
                }
                if self.text[i] == b'\\' && self.text[i..].starts_with(b"\\\"\"\"") {
                    out.push_str("\"\"\"");
                    i += 4;
                    continue;
                }
                out.push(self.text[i] as char);
                i += 1;
            }
            self.push(Tok::StrLit(out), s, i);
            return i;
        }
        i += 1;
        let mut out = String::new();
        loop {
            if i >= self.text.len() {
                self.errs.push(Diagnostic::error(
                    "E0003",
                    Span::new(s, i),
                    "unterminated string literal",
                ));
                return i;
            }
            let c = self.text[i];
            if c == b'"' {
                i += 1;
                break;
            }
            if c == b'\n' {
                self.errs.push(Diagnostic::error(
                    "E0003",
                    Span::new(s, i),
                    "unterminated string literal",
                ));
                return i;
            }
            if c == b'\\' {
                i += 1;
                if i >= self.text.len() {
                    break;
                }
                match self.text[i] {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'\'' => out.push('\''),
                    b'0' => out.push('\0'),
                    b'u' => {
                        // \u{HEX}
                        if self.peek(i + 1) == Some(b'{') {
                            let mut j = i + 2;
                            let hs = j;
                            while j < self.text.len() && self.text[j] != b'}' {
                                j += 1;
                            }
                            if j >= self.text.len() {
                                self.errs.push(Diagnostic::error(
                                    "E0010",
                                    Span::new(s, j),
                                    "unterminated unicode escape",
                                ));
                                return j;
                            }
                            let hex = String::from_utf8_lossy(&self.text[hs..j]).into_owned();
                            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                Some(ch) => out.push(ch),
                                None => self.errs.push(Diagnostic::error(
                                    "E0010",
                                    Span::new(s, j + 1),
                                    "invalid unicode escape",
                                )),
                            }
                            i = j + 1;
                            continue;
                        } else {
                            self.errs.push(Diagnostic::error(
                                "E0010",
                                Span::new(s, i + 1),
                                "invalid escape",
                            ));
                        }
                    }
                    _ => {
                        self.errs.push(Diagnostic::error(
                            "E0010",
                            Span::new(s, i + 1),
                            format!("unknown escape `\\{}`", self.text[i] as char),
                        ));
                    }
                }
                i += 1;
                continue;
            }
            out.push(c as char);
            i += 1;
        }
        self.push(Tok::StrLit(out), s, i);
        i
    }

    fn char_lit(&mut self, s: usize) -> usize {
        let mut i = s + 1;
        if i >= self.text.len() {
            self.errs.push(Diagnostic::error(
                "E0003",
                Span::new(s, i),
                "unterminated character literal",
            ));
            return i;
        }
        let ch: char;
        if self.text[i] == b'\\' {
            i += 1;
            if i >= self.text.len() {
                self.errs.push(Diagnostic::error(
                    "E0003",
                    Span::new(s, i),
                    "unterminated character literal",
                ));
                return i;
            }
            ch = match self.text[i] {
                b'n' => '\n',
                b't' => '\t',
                b'\\' => '\\',
                b'\'' => '\'',
                b'"' => '"',
                b'0' => '\0',
                _ => {
                    self.errs.push(Diagnostic::error(
                        "E0010",
                        Span::new(s, i + 1),
                        "invalid char escape",
                    ));
                    return i + 1;
                }
            };
            i += 1;
        } else if self.text[i].is_ascii_alphabetic() || self.text[i] == b'_' {
            // Possible lifetime `'abc` vs char `'a'`: a single ident char
            // followed by `'` is a char literal (Rust's rule); otherwise this
            // is a lifetime token and the quote (if any) lexes separately.
            let mut j = i;
            while j < self.text.len()
                && (self.text[j].is_ascii_alphanumeric() || self.text[j] == b'_')
            {
                j += 1;
            }
            if j == i + 1 && self.peek(j) == Some(b'\'') {
                ch = self.text[i] as char;
                i += 2;
                self.push(Tok::CharLit(ch), s, i);
                return i;
            }
            let name = String::from_utf8_lossy(&self.text[i..j]).into_owned();
            self.push(Tok::Lifetime(name), s, j);
            return j;
        } else {
            ch = self.text[i] as char;
            i += 1;
        }
        if self.peek(i) != Some(b'\'') {
            self.errs.push(Diagnostic::error(
                "E0010",
                Span::new(s, i),
                "character literal must contain exactly one character",
            ));
            return i;
        }
        i += 1;
        self.push(Tok::CharLit(ch), s, i);
        i
    }

    fn symbol(&mut self, i: usize) -> usize {
        let s = i;
        let two = if i + 1 < self.text.len() {
            Some((self.text[i], self.text[i + 1]))
        } else {
            None
        };
        let three = if i + 2 < self.text.len() {
            Some((self.text[i], self.text[i + 1], self.text[i + 2]))
        } else {
            None
        };
        macro_rules! tok2 {
            ($t:expr) => {{
                self.push($t, s, s + 2);
                return s + 2;
            }};
        }
        // &&mut? `&mut` is lexed as AmpMut when followed by ident/boundary.
        if three == Some((b'&', b'm', b'u')) {
            // handled below with lookahead for "mut"
        }
        if three == Some((b'.', b'.', b'=')) {
            self.push(Tok::DotDotEq, s, s + 3);
            return s + 3;
        }
        match two {
            Some((b'=', b'=')) => tok2!(Tok::EqEq),
            Some((b'!', b'=')) => tok2!(Tok::BangEq),
            Some((b'<', b'=')) => tok2!(Tok::LtEq),
            Some((b'>', b'=')) => tok2!(Tok::GtEq),
            Some((b'&', b'&')) => tok2!(Tok::AndAnd),
            Some((b'|', b'|')) => tok2!(Tok::OrOr),
            Some((b'|', b'>')) => tok2!(Tok::Pipe),
            Some((b'-', b'>')) => tok2!(Tok::Arrow),
            Some((b'+', b'=')) => tok2!(Tok::PlusEq),
            Some((b'-', b'=')) => tok2!(Tok::MinusEq),
            Some((b'*', b'=')) => tok2!(Tok::StarEq),
            Some((b'/', b'=')) => tok2!(Tok::SlashEq),
            Some((b'%', b'=')) => tok2!(Tok::PercentEq),
            Some((b'?', b'.')) => tok2!(Tok::DotQuestion),
            Some((b':', b':')) => tok2!(Tok::ColonColon),
            Some((b'.', b'.')) => tok2!(Tok::DotDot),
            _ => {}
        }
        if self.text[s..].starts_with(b"&mut")
            && (s + 4 >= self.text.len() || !is_ident(self.text[s + 4]) && self.text[s + 4] != b'_')
        {
            // ensure word boundary after mut (avoid &mutable)
            let after = s + 4;
            if after < self.text.len()
                && (self.text[after].is_ascii_alphanumeric() || self.text[after] == b'_')
            {
                // fall through to single &
            } else {
                self.push(Tok::AmpMut, s, s + 4);
                return s + 4;
            }
        }
        let t = match self.text[s] {
            b'+' => Tok::Plus,
            b'-' => Tok::Minus,
            b'*' => Tok::Star,
            b'/' => Tok::Slash,
            b'%' => Tok::Percent,
            b'<' => Tok::Lt,
            b'>' => Tok::Gt,
            b'!' => Tok::Bang,
            b'=' => Tok::Eq,
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b'{' => Tok::LBrace,
            b'}' => Tok::RBrace,
            b'[' => Tok::LBracket,
            b']' => Tok::RBracket,
            b',' => Tok::Comma,
            b':' => Tok::Colon,
            b';' => Tok::Semi,
            b'.' => Tok::Dot,
            b'?' => Tok::Question,
            b'&' => Tok::Amp,
            b'|' => Tok::BitOr,
            b'@' => Tok::At,
            b'^' => Tok::Caret,
            _ => {
                let ch = self.text[s] as char;
                self.errs.push(
                    Diagnostic::error(
                        "E0001",
                        Span::new(s, s + 1),
                        format!("invalid character `{}`", ch.escape_default()),
                    )
                    .with_help("remove or replace this character"),
                );
                return s + 1;
            }
        };
        self.push(t, s, s + 1);
        s + 1
    }
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn is_digit_base(c: u8, base: u32) -> bool {
    if c == b'_' {
        return true;
    }
    match base {
        16 => c.is_ascii_hexdigit(),
        10 => c.is_ascii_digit(),
        8 => matches!(c, b'0'..=b'7'),
        2 => matches!(c, b'0'..=b'1'),
        _ => false,
    }
}
