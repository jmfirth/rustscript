//! User-facing compiler diagnostics.
//!
//! [`Diagnostic`] represents a structured message about a problem (or note)
//! in the user's `.rts` source code. Diagnostics are accumulated during
//! compilation and rendered with source context via [`render_diagnostics`].
//!
//! This module deliberately does **not** expose `codespan-reporting` types
//! in its public API — the conversion happens internally during rendering.

use crate::source::{FileId, SourceMap};
use crate::span::Span;

/// Controls whether diagnostic output uses ANSI colors.
///
/// Passed through the pipeline from the CLI `--color` flag. The `Auto` variant
/// defers to terminal detection (performed at the CLI layer before passing to
/// lower crates).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Always emit ANSI color codes.
    Always,
    /// Never emit ANSI color codes (plain text).
    #[default]
    Never,
}

/// The severity level of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A fatal error that prevents compilation.
    Error,
    /// A warning that does not prevent compilation but indicates a likely problem.
    Warning,
    /// An informational note, often attached to another diagnostic.
    Note,
}

/// A label pointing to a span of source code within a diagnostic.
///
/// Labels provide source context — they highlight a region of code and
/// attach a message explaining the significance of that region.
#[derive(Debug, Clone)]
pub struct Label {
    /// The source region this label highlights.
    pub span: Span,
    /// The file containing the highlighted source.
    pub file_id: FileId,
    /// A message describing why this region is relevant.
    pub message: String,
}

/// A compiler diagnostic — a user-facing message about a problem in source code.
///
/// Diagnostics are built using a builder pattern:
///
/// ```
/// use rustscript_syntax::diagnostic::Diagnostic;
/// use rustscript_syntax::span::Span;
/// use rustscript_syntax::source::FileId;
///
/// let d = Diagnostic::error("type mismatch")
///     .with_label(Span::new(0, 5), FileId(0), "expected `i32`")
///     .with_note("consider adding a type annotation");
/// ```
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// How severe this diagnostic is.
    pub severity: Severity,
    /// The primary human-readable message.
    pub message: String,
    /// Labels pointing to relevant source regions.
    pub labels: Vec<Label>,
    /// Additional notes displayed after the source context.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// Create an error diagnostic with the given message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Create a warning diagnostic with the given message.
    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    /// Attach a label highlighting a source span.
    #[must_use]
    pub fn with_label(mut self, span: Span, file_id: FileId, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            file_id,
            message: message.into(),
        });
        self
    }

    /// Attach an additional note to this diagnostic.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// Render diagnostics to a writer with source context (no colors).
///
/// Convenience wrapper around [`render_diagnostics_colored`] with
/// [`ColorMode::Never`]. Use this when the caller does not have color
/// configuration (e.g., in tests or plain-text contexts).
///
/// # Errors
///
/// Returns an I/O error if writing to the provided writer fails.
pub fn render_diagnostics(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
    writer: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    render_diagnostics_colored(diagnostics, source_map, writer, ColorMode::Never)
}

/// Emit all diagnostics through a `WriteColor` implementor.
///
/// Internal helper shared by [`render_diagnostics_colored`]. Converts
/// our diagnostic types to `codespan-reporting` equivalents and renders
/// them with the given writer.
fn emit_all_diagnostics<W: codespan_reporting::term::termcolor::WriteColor>(
    writer: &mut W,
    config: &codespan_reporting::term::Config,
    files: &codespan_reporting::files::SimpleFiles<&str, &str>,
    diagnostics: &[Diagnostic],
    id_map: &[usize],
) -> std::io::Result<()> {
    use codespan_reporting::diagnostic as cs_diag;

    for diag in diagnostics {
        let severity = match diag.severity {
            Severity::Error => cs_diag::Severity::Error,
            Severity::Warning => cs_diag::Severity::Warning,
            Severity::Note => cs_diag::Severity::Note,
        };

        let labels: Vec<cs_diag::Label<usize>> = diag
            .labels
            .iter()
            .filter_map(|label| {
                let cs_file_id = id_map.get(label.file_id.0 as usize).copied()?;
                Some(
                    cs_diag::Label::primary(
                        cs_file_id,
                        (label.span.start.0 as usize)..(label.span.end.0 as usize),
                    )
                    .with_message(&label.message),
                )
            })
            .collect();

        let cs_diag = cs_diag::Diagnostic::new(severity)
            .with_message(&diag.message)
            .with_labels(labels)
            .with_notes(diag.notes.clone());

        codespan_reporting::term::emit(writer, config, files, &cs_diag)
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}

/// Render diagnostics to a writer with source context and optional ANSI colors.
///
/// Converts our internal diagnostic types to `codespan-reporting` types
/// and renders them. When `color` is [`ColorMode::Always`], the output
/// includes ANSI color codes for error/warning/note labels, source line
/// numbers, caret underlines, and file paths. When [`ColorMode::Never`],
/// the output is plain text.
///
/// The `codespan-reporting` dependency does not leak into the public API.
///
/// # Errors
///
/// Returns an I/O error if writing to the provided writer fails.
pub fn render_diagnostics_colored(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
    writer: &mut dyn std::io::Write,
    color: ColorMode,
) -> std::io::Result<()> {
    use codespan_reporting::files::SimpleFiles;
    use codespan_reporting::term;
    use codespan_reporting::term::termcolor::{Ansi, ColorSpec, NoColor, WriteColor};

    // Build a SimpleFiles store from our SourceMap.
    let mut files = SimpleFiles::new();

    // Map from our FileId to codespan's file id.
    // We iterate through file ids 0..N and add each one.
    let mut id_map = Vec::new();
    let mut file_idx = 0u32;
    loop {
        let our_id = crate::source::FileId(file_idx);
        let Some(sf) = source_map.get_file(our_id) else {
            break;
        };
        let cs_id = files.add(sf.name(), sf.source());
        id_map.push(cs_id);
        file_idx += 1;
    }

    let config = term::Config::default();

    match color {
        ColorMode::Always => {
            // Ansi<W> from termcolor wraps any io::Write with ANSI escape codes.
            let mut ansi_writer = Ansi::new(writer);
            emit_all_diagnostics(&mut ansi_writer, &config, &files, diagnostics, &id_map)?;
            // Ensure the color is reset after all diagnostics.
            ansi_writer.set_color(&ColorSpec::new())?;
            ansi_writer.reset()?;
        }
        ColorMode::Never => {
            let mut no_color = NoColor::new(writer);
            emit_all_diagnostics(&mut no_color, &config, &files, diagnostics, &id_map)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_error_creates_error_severity() {
        let d = Diagnostic::error("something went wrong");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, "something went wrong");
        assert!(d.labels.is_empty());
        assert!(d.notes.is_empty());
    }

    #[test]
    fn test_diagnostic_warning_creates_warning_severity() {
        let d = Diagnostic::warning("unused variable");
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.message, "unused variable");
    }

    #[test]
    fn test_diagnostic_with_label_and_with_note_chain() {
        let d = Diagnostic::error("type mismatch")
            .with_label(Span::new(0, 5), FileId(0), "expected i32")
            .with_label(Span::new(10, 15), FileId(0), "found string")
            .with_note("consider adding a type annotation");

        assert_eq!(d.labels.len(), 2);
        assert_eq!(d.labels[0].message, "expected i32");
        assert_eq!(d.labels[0].span, Span::new(0, 5));
        assert_eq!(d.labels[0].file_id, FileId(0));
        assert_eq!(d.labels[1].message, "found string");
        assert_eq!(d.notes.len(), 1);
        assert_eq!(d.notes[0], "consider adding a type annotation");
    }

    #[test]
    fn test_render_diagnostics_produces_output_with_error_and_source() {
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("test.rts".into(), "let x = 42;\n".into());

        let d = Diagnostic::error("unexpected token").with_label(Span::new(4, 5), file_id, "here");

        let mut output = Vec::new();
        render_diagnostics(&[d], &sm, &mut output).expect("render should not fail");
        let text = String::from_utf8(output).expect("output should be utf-8");

        // The rendered output should contain the error message and the source line.
        assert!(
            text.contains("unexpected token"),
            "output should contain the error message, got:\n{text}"
        );
        assert!(
            text.contains("let x = 42;"),
            "output should contain the source line, got:\n{text}"
        );
        assert!(
            text.contains("test.rts"),
            "output should contain the file name, got:\n{text}"
        );
    }

    #[test]
    fn test_render_diagnostics_empty_list_produces_no_output() {
        let sm = SourceMap::new();
        let mut output = Vec::new();
        render_diagnostics(&[], &sm, &mut output).expect("render should not fail");
        assert!(output.is_empty());
    }

    #[test]
    fn test_render_diagnostics_colored_always_emits_ansi_codes() {
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("test.rts".into(), "let x = 42;\n".into());

        let d = Diagnostic::error("unexpected token").with_label(Span::new(4, 5), file_id, "here");

        let mut output = Vec::new();
        render_diagnostics_colored(&[d], &sm, &mut output, ColorMode::Always)
            .expect("render should not fail");
        let text = String::from_utf8(output).expect("output should be utf-8");

        // ANSI escape codes start with ESC (0x1b) followed by '['.
        assert!(
            text.contains("\x1b["),
            "colored output should contain ANSI escape codes, got:\n{text}"
        );
        assert!(
            text.contains("unexpected token"),
            "output should still contain the error message, got:\n{text}"
        );
    }

    #[test]
    fn test_render_diagnostics_colored_never_omits_ansi_codes() {
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("test.rts".into(), "let x = 42;\n".into());

        let d = Diagnostic::error("unexpected token").with_label(Span::new(4, 5), file_id, "here");

        let mut output = Vec::new();
        render_diagnostics_colored(&[d], &sm, &mut output, ColorMode::Never)
            .expect("render should not fail");
        let text = String::from_utf8(output).expect("output should be utf-8");

        assert!(
            !text.contains("\x1b["),
            "plain output should NOT contain ANSI escape codes, got:\n{text}"
        );
        assert!(
            text.contains("unexpected token"),
            "output should contain the error message, got:\n{text}"
        );
    }

    #[test]
    fn test_render_diagnostics_colored_warning_produces_output() {
        let mut sm = SourceMap::new();
        let file_id = sm.add_file("test.rts".into(), "let x = 42;\n".into());

        let d = Diagnostic::warning("unused variable").with_label(
            Span::new(4, 5),
            file_id,
            "this variable",
        );

        let mut output = Vec::new();
        render_diagnostics_colored(&[d], &sm, &mut output, ColorMode::Always)
            .expect("render should not fail");
        let text = String::from_utf8(output).expect("output should be utf-8");

        assert!(
            text.contains("unused variable"),
            "output should contain the warning message, got:\n{text}"
        );
        assert!(
            text.contains("\x1b["),
            "colored output should contain ANSI escape codes, got:\n{text}"
        );
    }
}
