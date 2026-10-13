use crate::source::{Source, Span};

/// Structured compiler diagnostic. User errors are values, never panics.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub help: Option<String>,
    pub span: Span,
    pub runtime: bool,
    /// Owning source file (path as given to the compiler). `None` in
    /// single-source mode (rendered against the only source); stamped by the
    /// multi-file pipeline (parser per file, sema per module) so errors render
    /// against the right file.
    pub file: Option<String>,
    /// Non-fatal warning (`warning[W....]`; compilation succeeds).
    pub warning: bool,
    /// Secondary labeled spans, rendered as `note:` sub-diagnostics
    /// against the same source (e.g. "value moved here" for E0301).
    /// Same-file by construction: labels point into the function body
    /// that owns the primary span.
    pub labels: Vec<(Span, String)>,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            help: None,
            span,
            runtime: false,
            file: None,
            warning: false,
            labels: vec![],
        }
    }
    /// Non-fatal diagnostic (compilation succeeds; printed on success).
    /// Warnings render as `warning[W....]` and never fail gates by themselves.
    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            help: None,
            span,
            runtime: false,
            file: None,
            warning: true,
            labels: vec![],
        }
    }
    /// Attribute this diagnostic to a source file (multi-file mode).
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
    /// Attribute a secondary labeled span (rendered as `note:`).
    pub fn with_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.labels.push((span, label.into()));
        self
    }
    pub fn render(&self, source: &Source) -> String {
        let (line, col) = source.line_col(self.span.start);
        let kind = if self.warning { "warning" } else { "error" };
        let mut out = format!(
            "{}[{}]: {} --> {}:{}:{}",
            kind, self.code, self.message, source.name, line, col
        );
        // Source snippet with caret underline (single line; multi-line
        // spans clamp to the first line). Never panics on bad spans.
        let text = source.line_text(line);
        if !text.is_empty() {
            let lstart = source.line_start(line);
            let lend = lstart + text.len();
            let s = self.span.start.clamp(lstart, lend);
            let e = self.span.end.clamp(s, lend);
            let caret_col = source
                .text
                .get(lstart..s)
                .map(|t| t.chars().count())
                .unwrap_or(0);
            let width = source
                .text
                .get(s..e)
                .map(|t| t.chars().count().max(1))
                .unwrap_or(1);
            let gutter = line.to_string().len();
            out.push_str(&format!("\n{} | {}", line, text));
            out.push_str(&format!(
                "\n{} | {}{}",
                " ".repeat(gutter),
                " ".repeat(caret_col),
                "^".repeat(width)
            ));
        }
        for (span, label) in &self.labels {
            let (lline, lcol) = source.line_col(span.start);
            let ltext = source.line_text(lline);
            out.push_str(&format!(
                "\n  note: {} --> {}:{}:{}",
                label, source.name, lline, lcol
            ));
            if !ltext.is_empty() {
                let ls = source.line_start(lline);
                let le = ls + ltext.len();
                let s = span.start.clamp(ls, le);
                let e = span.end.clamp(s, le);
                let caret_col = source
                    .text
                    .get(ls..s)
                    .map(|t| t.chars().count())
                    .unwrap_or(0);
                let width = source
                    .text
                    .get(s..e)
                    .map(|t| t.chars().count().max(1))
                    .unwrap_or(1);
                let gutter = lline.to_string().len();
                out.push_str(&format!("\n{} | {}", lline, ltext));
                out.push_str(&format!(
                    "\n{} | {}{}",
                    " ".repeat(gutter),
                    " ".repeat(caret_col),
                    "^".repeat(width)
                ));
            }
        }
        if let Some(h) = &self.help {
            out.push_str(&format!("\n  help: {}", h));
        }
        out
    }
}

/// Internal compiler bug (genuine invariant violation only).
#[derive(Debug)]
pub struct Ice(pub String);
