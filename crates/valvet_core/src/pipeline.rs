use crate::ast::Program;
use crate::diag::Diagnostic;
use crate::sema::Checked;
use crate::source::Source;

/// Lex + parse one source unit (no typechecking). Used by the file-based
/// resolver (each module parses separately so errors attribute per file).
pub fn parse_unit(source: &Source) -> Result<Program, Vec<Diagnostic>> {
    let (toks, diags) = crate::lexer::lex(source);
    if !diags.is_empty() {
        return Err(diags);
    }
    let (prog, perrs) = crate::parser::Parser::new(toks).parse();
    if !perrs.is_empty() {
        return Err(perrs);
    }
    Ok(prog.expect("parser returned no program and no errors"))
}

/// Full front-end pipeline: lex -> parse -> typecheck -> ownership.
/// Returns checked HIR or the diagnostic list. Never panics on user input.
/// Single-source mode: `import` declarations are rejected (E0701 — imports
/// need file-based compilation via `front_end_file`).
pub fn front_end(source: &Source) -> Result<Checked, Vec<Diagnostic>> {
    let mut prog = parse_unit(source)?;
    // Single-source is always the entry: same script-mode synthesis as
    // the file pipeline, then the same id renumbering (synthesis uses
    // placeholder ids).
    let mut derrs = vec![];
    if !crate::resolver::apply_entry_script(&mut prog, &source.name, &mut derrs) {
        return Err(derrs);
    }
    crate::resolver::renumber(&mut prog);
    if !prog.imports.is_empty() {
        let imp = &prog.imports[0];
        return Err(vec![Diagnostic::error(
            "E0701",
            imp.span,
            "imports need file-based compilation (`valvet check path/to/main.valvet`)",
        )]);
    }
    check_unit(&prog, vec![source.clone()])
}

/// Shared tail of both pipelines: sema + ownership over one joint program.
/// Stamps owning sources for warning rendering on success.
fn check_unit(prog: &Program, files: Vec<Source>) -> Result<Checked, Vec<Diagnostic>> {
    let mut diags = vec![];
    let (checked_opt, serrs) = crate::sema::check(prog);
    diags.extend(serrs);
    let mut checked = match checked_opt {
        Some(c) => c,
        None => return Err(diags),
    };
    let oerrs = crate::ownership::analyze(&checked);
    if !oerrs.is_empty() {
        diags.extend(oerrs);
        return Err(diags);
    }
    checked.files = files;
    // `mode exp` denies warnings: the same codes surface as errors
    // (fixture-safe: the harness matches codes only).
    if checked.program.mode == crate::ast::Mode::Exp && !checked.warnings.is_empty() {
        for w in &mut checked.warnings {
            w.warning = false;
        }
        diags.extend(std::mem::take(&mut checked.warnings));
        return Err(diags);
    }
    Ok(checked)
}

/// Multi-file diagnostics: every diagnostic carries `.file` (the display
/// path); `files` holds the loaded sources for line/col rendering.
#[derive(Debug)]
pub struct FileDiags {
    pub files: Vec<Source>,
    pub diags: Vec<Diagnostic>,
}

impl FileDiags {
    pub fn codes(&self) -> Vec<&str> {
        self.diags.iter().map(|d| d.code).collect()
    }

    /// Render all diagnostics against their owning files (falls back to a
    /// file-only header when the source is unavailable, e.g. unreadable
    /// entry — never panics).
    pub fn render(&self) -> String {
        self.diags
            .iter()
            .map(|d| {
                let kind = if d.warning { "warning" } else { "error" };
                match &d.file {
                    Some(f) => match self.files.iter().find(|s| &s.name == f) {
                        Some(src) => d.render(src),
                        None => format!("{}[{}]: {} --> {}", kind, d.code, d.message, f),
                    },
                    None => match self.files.first() {
                        Some(src) => d.render(src),
                        None => format!("{}[{}]: {}", kind, d.code, d.message),
                    },
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// File-based front-end (Phase H): load the entry file plus transitive
/// imports from its directory, then joint-check all units (shared
/// signatures; "Env per module" scoping inside sema). Single C TU downstream.
pub fn front_end_file(entry: &std::path::Path) -> Result<Checked, FileDiags> {
    let (krate, mut derrs) = crate::resolver::load(entry);
    let files: Vec<Source> = krate.sources.clone();
    if !derrs.is_empty() {
        return Err(FileDiags {
            files,
            diags: derrs,
        });
    }
    let prog = krate.joint_renumbered();
    let mod_files: std::collections::HashMap<String, String> = krate
        .units
        .iter()
        .map(|u| (u.module.clone(), u.display.clone()))
        .collect();
    let (checked_opt, serrs) = crate::sema::check_with_files(&prog, &mod_files);
    derrs.extend(serrs);
    let mut checked = match checked_opt {
        Some(c) => c,
        None => {
            return Err(FileDiags {
                files,
                diags: derrs,
            });
        }
    };
    let oerrs = crate::ownership::analyze_with_files(&checked, &mod_files);
    if !oerrs.is_empty() {
        derrs.extend(oerrs);
        return Err(FileDiags {
            files,
            diags: derrs,
        });
    }
    checked.files = files;
    // `mode exp` denies warnings (same as the single-source tail above).
    if checked.program.mode == crate::ast::Mode::Exp && !checked.warnings.is_empty() {
        for w in &mut checked.warnings {
            w.warning = false;
        }
        derrs.extend(std::mem::take(&mut checked.warnings));
        return Err(FileDiags {
            files: std::mem::take(&mut checked.files),
            diags: derrs,
        });
    }
    Ok(checked)
}

/// Render user diagnostics for terminal output.
pub fn render_diags(source: &Source, diags: &[Diagnostic]) -> String {
    diags
        .iter()
        .map(|d| d.render(source))
        .collect::<Vec<_>>()
        .join("\n")
}
