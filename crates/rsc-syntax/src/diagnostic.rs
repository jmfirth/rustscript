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
/// use rsc_syntax::diagnostic::Diagnostic;
/// use rsc_syntax::span::Span;
/// use rsc_syntax::source::FileId;
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

/// Render diagnostics to a writer with source context.
///
/// Converts our internal diagnostic types to `codespan-reporting` types
/// and renders them. The `codespan-reporting` dependency does not leak
/// into the public API.
///
/// # Errors
///
/// Returns an I/O error if writing to the provided writer fails.
pub fn render_diagnostics(
    diagnostics: &[Diagnostic],
    source_map: &SourceMap,
    writer: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    use codespan_reporting::diagnostic as cs_diag;
    use codespan_reporting::files::SimpleFiles;
    use codespan_reporting::term;
    use codespan_reporting::term::termcolor::{ColorSpec, WriteColor};

    // Wrap the writer in a NoColor adapter so codespan-reporting can use it.
    struct WriterAdapter<'a>(&'a mut dyn std::io::Write);

    impl std::io::Write for WriterAdapter<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    impl WriteColor for WriterAdapter<'_> {
        fn supports_color(&self) -> bool {
            false
        }

        fn set_color(&mut self, _spec: &ColorSpec) -> std::io::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

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
    let mut adapter = WriterAdapter(writer);

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

        term::emit(&mut adapter, &config, &files, &cs_diag).map_err(std::io::Error::other)?;
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
}
