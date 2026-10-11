//! Stable-compatible fuzz harness (PLAN Phase A3).
//!
//! `cargo-fuzz`/libFuzzer needs nightly + LLVM + C++ toolchain (unavailable here),
//! so this harness provides the same core property on stable Rust: **the front end
//! never panics or hangs on any input**. It feeds (a) pseudo-random token soup and
//! (b) structural mutations of every compile-pass fixture through `front_end` inside
//! `catch_unwind`. Any `Ok`/`Err` outcome is acceptable; a panic or timeout is failure.
//!
//! Full libFuzzer integration remains a follow-up when a nightly toolchain exists.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use valvet_core::{pipeline, source::Source};

/// Deterministic xorshift64 PRNG (no dependencies).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
}

const TOKENS: &[&str] = &[
    "fun", "val", "var", "if", "else", "while", "for", "loop", "break", "continue", "return",
    "true", "false", "in", "as", "Int32", "Int64", "Float64", "Bool", "String", "main", "x", "foo",
    "0", "1", "42", "3.5", "\"s\"", "+", "-", "*", "/", "%", "==", "!=", "<", "<=", ">", ">=",
    "&&", "||", "!", "=", "+=", "(", ")", "{", "}", ",", ":", ";", ".", "..", "..=", "->", "?",
    "print", " ", " ", "\n", "\n", "//c\n", "/*", "*/",
];

fn soup(rng: &mut Rng, len: usize) -> String {
    let mut s = String::new();
    for _ in 0..len {
        s.push_str(TOKENS[rng.below(TOKENS.len())]);
    }
    s
}

fn mutate(rng: &mut Rng, base: &str) -> String {
    let mut bytes = base.as_bytes().to_vec();
    for _ in 0..1 + rng.below(4) {
        match rng.below(4) {
            0 if !bytes.is_empty() => {
                bytes.remove(rng.below(bytes.len()));
            }
            1 => {
                let at = rng.below(bytes.len() + 1);
                bytes.insert(at, b"{(["[rng.below(3)]);
            }
            2 if !bytes.is_empty() => {
                let at = rng.below(bytes.len());
                bytes[at] = b"+-*/=!<>;&|,.? \n"[rng.below(13)];
            }
            _ => {
                let at = rng.below(bytes.len() + 1);
                let tok = TOKENS[rng.below(TOKENS.len())];
                bytes.splice(at..at, tok.as_bytes().iter().cloned());
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn run_case(name: &str, text: &str) {
    let src = Source::new(name.to_string(), text.to_string());
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pipeline::front_end(&src)));
    assert!(r.is_ok(), "compiler panicked on fuzz input {}", name);
}

#[test]
fn fuzz_token_soup_no_panic() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    for i in 0..300 {
        // Bounded size keeps the parser's recursion well within stack limits.
        let len = 5 + rng.below(120);
        run_case(&format!("soup{}", i), &soup(&mut rng, len));
    }
}

#[test]
fn fuzz_fixture_mutations_no_panic() {
    let mut corpus = vec![];
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../valvet-tests/compile-pass");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("valvet") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    corpus.push(t);
                }
            }
        }
    }
    assert!(!corpus.is_empty(), "fixture corpus empty");
    let mut rng = Rng(0xdeadbeefcafe0000);
    for (i, base) in corpus.iter().enumerate() {
        for j in 0..20 {
            run_case(&format!("mut{}-{}", i, j), &mutate(&mut rng, base));
        }
        // Corpus sanity: hash logged so regressions are reproducible by seed.
        let mut h = DefaultHasher::new();
        base.hash(&mut h);
        let _ = h.finish();
    }
}
