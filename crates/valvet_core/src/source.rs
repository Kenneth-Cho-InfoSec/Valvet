/// Source file identity + line/col computation (1-based, Unicode-scalar cols).
#[derive(Debug, Clone)]
pub struct Source {
    pub name: String,
    pub text: String,
    line_starts: Vec<usize>,
}

impl Source {
    pub fn new(name: String, text: String) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            name,
            text,
            line_starts,
        }
    }

    /// Byte offset -> (line, col), both 1-based.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let off = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&off) {
            Ok(l) => l + 1,
            Err(l) => l,
        };
        let start = self.line_starts[line - 1];
        let col = self.text[start..off].chars().count() + 1;
        (line, col)
    }
}

/// Span of source bytes [start, end).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    pub fn empty(at: usize) -> Self {
        Self { start: at, end: at }
    }
}
