//! The one failure type the CLI renders. The engine has no report to fill; a build tool is
//! read through its diagnostics, so every blocking finding and every warning is one of these
//! with the file and line of the thing it names.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn error(file: impl Into<String>, line: u32, message: impl Into<String>) -> Self {
        Self {
            file: Some(file.into()),
            line: Some(line),
            message: message.into(),
            severity: Severity::Error,
        }
    }

    pub fn warning(file: Option<String>, line: Option<u32>, message: impl Into<String>) -> Self {
        Self {
            file,
            line,
            message: message.into(),
            severity: Severity::Warning,
        }
    }

    /// `{file}:{line}: {message}` for errors and `{file}:{line}: warning: {message}` for
    /// warnings; the message alone when there is no file.
    pub fn render(&self) -> String {
        let prefix = match (&self.file, self.line) {
            (Some(file), Some(line)) => format!("{file}:{line}: "),
            (Some(file), None) => format!("{file}: "),
            _ => String::new(),
        };
        match self.severity {
            Severity::Error => format!("{prefix}{}", self.message),
            Severity::Warning => format!("{prefix}warning: {}", self.message),
        }
    }

    /// Anything that names a project file goes to stdout; a message with no file is not
    /// about a project file and goes to stderr whatever its severity.
    pub fn stdout(&self) -> bool {
        self.file.is_some()
    }
}
