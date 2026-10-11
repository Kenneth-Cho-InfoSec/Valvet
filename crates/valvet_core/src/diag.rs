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
    pub fn render(&self, source: &Source) -> String {
        let (line, col) = source.line_col(self.span.start);
        let kind = if self.warning { "warning" } else { "error" };
        let mut out = format!(
            "{}[{}]: {} --> {}:{}:{}",
            kind, self.code, self.message, source.name, line, col
        );
        if let Some(h) = &self.help {
            out.push_str(&format!("\n  help: {}", h));
        }
        out
    }
}

/// Internal compiler bug (genuine invariant violation only).
#[derive(Debug)]
pub struct Ice(pub String);
