use std::fmt;

#[derive(Debug, Clone)]
pub struct LoomError {
    pub message: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
}

impl LoomError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            col: None,
        }
    }

    pub fn with_location(mut self, line: usize, col: usize) -> Self {
        self.line = Some(line);
        self.col = Some(col);
        self
    }
}

impl fmt::Display for LoomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.col) {
            (Some(line), Some(col)) => write!(f, "Error at line {}, col {}: {}", line, col, self.message),
            _ => write!(f, "Error: {}", self.message),
        }
    }
}

impl std::error::Error for LoomError {}

pub type Result<T> = std::result::Result<T, LoomError>;
