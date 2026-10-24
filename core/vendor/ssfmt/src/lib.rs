//! # ssfmt
//!
//! Excel-compatible ECMA-376 number format codes for Rust.
//!
//! This crate provides parsing and formatting of spreadsheet number format codes,
//! matching Excel's actual behavior including undocumented quirks.
//!
//! ## Quick Start
//!
//! ```rust
//! use ssfmt::{format_default, NumberFormat, FormatOptions};
//!
//! // One-off formatting
//! let result = format_default(1234.56, "#,##0.00").unwrap();
//! assert_eq!(result, "1,234.56");
//!
//! // Compile once, format many
//! let fmt = NumberFormat::parse("#,##0.00").unwrap();
//! let opts = FormatOptions::default();
//! assert_eq!(fmt.format(1234.56, &opts), "1,234.56");
//! assert_eq!(fmt.format(9876.54, &opts), "9,876.54");
//! ```
//!
//! ## Format Code Syntax
//!
//! Format codes can have up to 4 sections separated by semicolons:
//! 1. Positive numbers
//! 2. Negative numbers
//! 3. Zero
//! 4. Text
//!
//! ### Number Placeholders
//! - `0` - Display digit or zero
//! - `#` - Display digit or nothing
//! - `?` - Display digit or space
//!
//! ### Date/Time Codes
//! - `yyyy` - Four-digit year
//! - `mm` - Two-digit month
//! - `dd` - Two-digit day
//! - `hh` - Two-digit hour
//! - `mm` - Two-digit minute (after hour)
//! - `ss` - Two-digit second
//!
//! ## Feature Flags
//!
//! - `chrono` (default) - Enable chrono type support
//! - `bigint` - Enable BigInt support for arbitrary precision integers

pub mod ast;
pub mod builtin_formats;
pub mod error;
pub mod options;
pub mod value;

pub mod date_serial;
mod hijri;

mod cache;
mod formatter;
mod locale;
pub mod parser;

// Re-exports will be added once types are defined:
pub use ast::{NumberFormat, Section};
pub use builtin_formats::{format_code_from_id, is_builtin_format_id};
pub use error::{FormatError, ParseError};
pub use locale::Locale;
pub use options::{DateSystem, FormatOptions};
pub use value::Value;

// Convenience functions

/// Parse and format a value in one call.
///
/// This function caches recently used format codes for efficiency.
pub fn format(value: f64, format_code: &str, opts: &FormatOptions) -> Result<String, ParseError> {
    let fmt = cache::get_or_parse(format_code)?;
    Ok(fmt.format(value, opts))
}

/// Format a value with default options (1900 date system, en-US locale).
///
/// This function caches recently used format codes for efficiency.
pub fn format_default(value: f64, format_code: &str) -> Result<String, ParseError> {
    let opts = FormatOptions::default();
    format(value, format_code, &opts)
}

/// Format a value using a built-in format ID.
///
/// Excel stores built-in format IDs (0-49) in .xlsx files. This function
/// looks up the format code for the given ID and formats the value.
///
/// # Arguments
/// * `value` - The numeric value to format
/// * `format_id` - The built-in format ID (e.g., 0 for "General", 14 for "m/d/yy")
/// * `opts` - Format options (date system, locale)
///
/// # Returns
/// * `Ok(String)` - The formatted value
/// * `Err(ParseError::InvalidFormatId)` - If the format ID is not a recognized built-in format
///
/// # Examples
/// ```
/// use ssfmt::{format_with_id, FormatOptions};
///
/// let opts = FormatOptions::default();
/// assert_eq!(format_with_id(1234.56, 0, &opts).unwrap(), "1234.56"); // General
/// assert_eq!(format_with_id(1234.56, 2, &opts).unwrap(), "1234.56"); // 0.00
/// ```
pub fn format_with_id(
    value: f64,
    format_id: u32,
    opts: &FormatOptions,
) -> Result<String, ParseError> {
    let format_code = format_code_from_id(format_id)
        .ok_or(ParseError::InvalidFormatId(format_id))?;
    format(value, format_code, opts)
}

/// Format a value using a built-in format ID with default options.
///
/// Convenience wrapper around `format_with_id` using default options
/// (1900 date system, en-US locale).
///
/// # Examples
/// ```
/// use ssfmt::format_with_id_default;
///
/// assert_eq!(format_with_id_default(1234.56, 0).unwrap(), "1234.56"); // General
/// assert_eq!(format_with_id_default(0.5, 10).unwrap(), "50.00%"); // 0.00%
/// ```
pub fn format_with_id_default(value: f64, format_id: u32) -> Result<String, ParseError> {
    let opts = FormatOptions::default();
    format_with_id(value, format_id, &opts)
}

// BigInt convenience functions (requires `bigint` feature)

/// Re-export BigInt type for convenience (requires `bigint` feature).
#[cfg(feature = "bigint")]
pub use num_bigint::BigInt;

/// Format a BigInt value with a format code.
///
/// For values within f64's safe integer range (±2^53), converts to f64 and uses
/// standard formatting. For larger values, uses string-based formatting to
/// preserve precision.
///
/// This function caches recently used format codes for efficiency.
///
/// # Example
/// ```ignore
/// use ssfmt::{format_bigint, BigInt, FormatOptions};
///
/// let big = BigInt::parse_bytes(b"123456822333333000", 10).unwrap();
/// let opts = FormatOptions::default();
/// let result = format_bigint(&big, "#,##0", &opts).unwrap();
/// assert_eq!(result, "123,456,822,333,333,000");
/// ```
#[cfg(feature = "bigint")]
pub fn format_bigint(
    value: &num_bigint::BigInt,
    format_code: &str,
    opts: &FormatOptions,
) -> Result<String, ParseError> {
    let fmt = cache::get_or_parse(format_code)?;
    Ok(fmt.format_bigint(value, opts))
}

/// Format a BigInt value with a format code using default options.
///
/// Convenience wrapper around `format_bigint` using default options
/// (1900 date system, en-US locale).
#[cfg(feature = "bigint")]
pub fn format_bigint_default(
    value: &num_bigint::BigInt,
    format_code: &str,
) -> Result<String, ParseError> {
    let opts = FormatOptions::default();
    format_bigint(value, format_code, &opts)
}
