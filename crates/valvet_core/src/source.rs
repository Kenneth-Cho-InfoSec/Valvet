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

    /// Byte offset of the start of 1-based `line`; text length if out of range.
    pub fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        self.line_starts
            .get(line - 1)
            .copied()
            .unwrap_or(self.text.len())
    }

    /// Full text of 1-based `line` without the trailing newline; empty
    /// string if out of range (rendering never panics on bad spans).
    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_start(line);
        let end = self.line_start(line + 1);
        self.text
            .get(start..end)
            .unwrap_or("")
            .trim_end_matches('\n')
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
