//! BigInt formatting for arbitrary precision integers.
//!
//! This module handles formatting of large integers that exceed f64's safe integer range (±2^53).
//! For values within the safe range, the regular f64 formatting path is used.
//! For values outside the safe range, string-based arithmetic is used to preserve precision.

use crate::ast::{FormatPart, Section};
use crate::error::FormatError;
use crate::options::FormatOptions;
use num_bigint::BigInt;

/// The maximum safe integer value for f64 (2^53 - 1)
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
/// The minimum safe integer value for f64 (-(2^53 - 1))
pub const MIN_SAFE_INTEGER: i64 = -9_007_199_254_740_991;

/// Check if a BigInt is within the safe f64 integer range.
pub fn is_safe_integer(n: &BigInt) -> bool {
    let min_safe = BigInt::from(MIN_SAFE_INTEGER);
    let max_safe = BigInt::from(MAX_SAFE_INTEGER);
    n >= &min_safe && n <= &max_safe
}

/// Format a BigInt value according to a format section.
///
/// For values within safe f64 range, converts to f64 and uses standard formatting.
/// For values outside safe range, uses string-based formatting to preserve precision.
pub fn format_bigint(
    value: &BigInt,
    section: &Section,
    opts: &FormatOptions,
) -> Result<String, FormatError> {
    // Check if value is within safe f64 range
    if is_safe_integer(value) {
        // Convert to f64 and use standard formatting
        let float_val: f64 = value.to_string().parse().unwrap_or(0.0);
        return super::format_number(float_val, section, opts);
    }

    // For large integers, use string-based formatting
    format_large_bigint(value, section, opts)
}

/// Format a BigInt value that exceeds f64's safe integer range.
/// Uses string-based arithmetic to preserve precision.
fn format_large_bigint(
    value: &BigInt,
    section: &Section,
    opts: &FormatOptions,
) -> Result<String, FormatError> {
    use num_bigint::Sign;

    let is_negative = value.sign() == Sign::Minus;
    let abs_value = if is_negative {
        -value.clone()
    } else {
        value.clone()
    };

    // Analyze the format to understand what we need to do
    let analysis = super::number::analyze_format(section);

    // Apply thousands scaling (trailing commas divide by 1000 each)
    let scaled_value = if analysis.thousands_scale > 0 {
        let divisor = BigInt::from(1000_u64).pow(analysis.thousands_scale as u32);
        &abs_value / &divisor
    } else {
        abs_value.clone()
    };

    // Convert to string for formatting
    let value_str = scaled_value.to_string();

    // Format the integer part
    let formatted_integer = format_bigint_integer(
        &value_str,
        &analysis.integer_placeholders,
        analysis.has_thousands_separator,
        &analysis.inline_literals,
        opts,
    );

    // Handle decimal places (for BigInt, decimal part is always 0)
    let decimal_places = analysis.decimal_places();
    let formatted = if decimal_places > 0 {
        let zeros = "0".repeat(decimal_places);
        format!(
            "{}{}{}",
            formatted_integer, opts.locale.decimal_separator, zeros
        )
    } else {
        formatted_integer
    };

    // Build prefix
    let mut result = String::new();
    for part in &analysis.prefix_parts {
        match part {
            FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => result.push_str(s),
            FormatPart::Locale(locale_code) => {
                if let Some(ref currency) = locale_code.currency {
                    result.push_str(currency);
                }
            }
            FormatPart::Percent => result.push('%'),
            _ => {}
        }
    }

    // Add the formatted number
    result.push_str(&formatted);

    // Build suffix
    for part in &analysis.suffix_parts {
        match part {
            FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => result.push_str(s),
            FormatPart::Locale(locale_code) => {
                if let Some(ref currency) = locale_code.currency {
                    result.push_str(currency);
                }
            }
            FormatPart::Percent => result.push('%'),
            _ => {}
        }
    }

    Ok(result)
}

/// Format the integer part of a BigInt as a string.
fn format_bigint_integer(
    value_str: &str,
    placeholders: &[crate::ast::DigitPlaceholder],
    use_thousands: bool,
    inline_literals: &[(usize, String)],
    opts: &FormatOptions,
) -> String {
    let value_digits: Vec<char> = value_str.chars().collect();

    let min_digits = placeholders.iter().filter(|p| p.is_required()).count();
    let output_len = value_digits.len().max(min_digits);

    // Build right-to-left into Vec, then reverse once
    let separator_count = if use_thousands { output_len / 3 } else { 0 };
    let literal_chars: usize = inline_literals.iter().map(|(_, s)| s.len()).sum();
    let estimated_capacity = output_len + separator_count + literal_chars;
    let mut chars = Vec::with_capacity(estimated_capacity);

    // Process from right to left (least significant first)
    for (digit_count, pos_from_right) in (0..output_len).enumerate() {
        let digit_index = value_digits.len() as isize - 1 - pos_from_right as isize;

        // Add thousands separator if needed (but not at position 0)
        if use_thousands && digit_count > 0 && digit_count % 3 == 0 {
            chars.push(opts.locale.thousands_separator);
        }

        // Check if there's an inline literal at this position
        let literals_at_pos: Vec<&str> = inline_literals
            .iter()
            .filter(|(pos, _)| *pos == pos_from_right)
            .map(|(_, s)| s.as_str())
            .collect();

        for literal_str in literals_at_pos.iter().rev() {
            for ch in literal_str.chars().rev() {
                chars.push(ch);
            }
        }

        if digit_index >= 0 {
            // We have a digit from the value
            chars.push(value_digits[digit_index as usize]);
        } else {
            // Use placeholder's empty character for padding
            let placeholder_index = placeholders.len() as isize - 1 - pos_from_right as isize;
            if placeholder_index >= 0 {
                let placeholder = placeholders[placeholder_index as usize];
                if let Some(c) = placeholder.empty_char() {
                    chars.push(c);
                }
            }
        }
    }

    // Handle the case where we have no digits but need at least one
    if chars.is_empty() && placeholders.iter().any(|p| p.is_required()) {
        chars.push('0');
    }

    // Push any inline literals that are at positions beyond what we formatted
    for (literal_pos, literal_str) in inline_literals {
        if *literal_pos >= output_len {
            for ch in literal_str.chars().rev() {
                chars.push(ch);
            }
        }
    }

    // Reverse and collect into String
    chars.reverse();
    chars.into_iter().collect()
}

/// Fallback formatting for BigInt values.
/// Converts to string representation.
pub fn fallback_format_bigint(value: &BigInt) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_integer() {
        assert!(is_safe_integer(&BigInt::from(0)));
        assert!(is_safe_integer(&BigInt::from(1000)));
        assert!(is_safe_integer(&BigInt::from(-1000)));
        assert!(is_safe_integer(&BigInt::from(MAX_SAFE_INTEGER)));
        assert!(is_safe_integer(&BigInt::from(MIN_SAFE_INTEGER)));

        // Just outside safe range
        let above_max = BigInt::from(MAX_SAFE_INTEGER) + 1;
        let below_min = BigInt::from(MIN_SAFE_INTEGER) - 1;
        assert!(!is_safe_integer(&above_max));
        assert!(!is_safe_integer(&below_min));

        // Large values
        assert!(!is_safe_integer(&BigInt::parse_bytes(b"123456822333333000", 10).unwrap()));
    }

    #[test]
    fn test_fallback_format_bigint() {
        let big = BigInt::parse_bytes(b"123456822333333000", 10).unwrap();
        assert_eq!(fallback_format_bigint(&big), "123456822333333000");
    }
}
