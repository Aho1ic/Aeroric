//! Error types for parsing and formatting.

use thiserror::Error;

/// Errors that can occur when parsing a format code.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParseError {
    #[error("unexpected token at position {position}: found '{found}'")]
    UnexpectedToken { position: usize, found: char },

    #[error("unterminated bracket at position {position}")]
    UnterminatedBracket { position: usize },

    #[error("invalid condition at position {position}: {reason}")]
    InvalidCondition { position: usize, reason: String },

    #[error("invalid locale code at position {position}")]
    InvalidLocaleCode { position: usize },

    #[error("too many sections (maximum 4 allowed)")]
    TooManySections,

    #[error("empty format code")]
    EmptyFormat,

    #[error("invalid format ID: {0} is not a recognized built-in format")]
    InvalidFormatId(u32),
}

/// Errors that can occur when formatting a value.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum FormatError {
    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: &'static str,
        got: &'static str,
    },

    #[error("date out of range: serial number {serial}")]
    DateOutOfRange { serial: f64 },

    #[error("invalid serial number: {value}")]
    InvalidSerialNumber { value: f64 },
}
