//! Fraction formatting

use crate::ast::{DigitPlaceholder, FormatPart, FractionDenom, Section};
use crate::error::FormatError;
use crate::formatter::number::format_simple_with_placeholders;
use crate::options::FormatOptions;

/// Format a fraction part (numerator or denominator) with digit placeholders.
/// Uses the unified placeholder formatting helper from number.rs.
fn format_fraction_part(value: u64, placeholders: &[DigitPlaceholder]) -> String {
    format_simple_with_placeholders(value, placeholders)
}

/// Format a number as a fraction according to the format section.
pub fn format_fraction(
    value: f64,
    section: &Section,
    _opts: &FormatOptions,
) -> Result<String, FormatError> {
    // Find the fraction part in the section
    let fraction_part = section.parts.iter().find_map(|p| {
        if let FormatPart::Fraction {
            integer_digits,
            numerator_digits,
            denominator,
            space_before_slash,
            space_after_slash,
        } = p
        {
            Some((integer_digits, numerator_digits, denominator, space_before_slash, space_after_slash))
        } else {
            None
        }
    });

    let Some((integer_digits, numerator_digits, denominator, space_before_slash, space_after_slash)) = fraction_part else {
        return Err(FormatError::TypeMismatch {
            expected: "fraction format",
            got: "no fraction part found",
        });
    };

    // Separate integer and fractional parts
    let abs_value = value.abs();
    let mut integer_part = abs_value.trunc() as i64;
    let frac_part = abs_value.fract();

    // Determine if this is a mixed fraction or improper fraction
    let is_mixed = !integer_digits.is_empty();

    // Calculate padding width (ri in SSF) - used for both numerator and denominator padding
    // For mixed fractions: Math.min(Math.max(numerator_len, denominator_len), 7)
    // For improper fractions: Math.min(denominator_len, 7)
    let padding_width = match denominator {
        FractionDenom::UpToDigits(denom_digits) => {
            if is_mixed {
                let numerator_len = numerator_digits.len() as u8;
                numerator_len.max(*denom_digits).min(7)
            } else {
                (*denom_digits).min(7)
            }
        }
        FractionDenom::Fixed(_) => {
            // For fixed denominators, no padding width calculation needed
            0
        }
    };

    // Find best fraction approximation
    let (mut num, denom) = if is_mixed {
        // Mixed fraction: approximate the fractional part only
        match denominator {
            FractionDenom::UpToDigits(_) => {
                let max_denom = 10_u32.pow(padding_width as u32) - 1;
                find_best_fraction(frac_part, max_denom)
            }
            FractionDenom::Fixed(d) => {
                let num = (frac_part * (*d as f64)).round() as u32;
                (num, *d)
            }
        }
    } else {
        // Improper fraction: approximate the entire value
        match denominator {
            FractionDenom::UpToDigits(_) => {
                let max_denom = 10_u32.pow(padding_width as u32) - 1;
                find_best_fraction(abs_value, max_denom)
            }
            FractionDenom::Fixed(d) => {
                let num = (abs_value * (*d as f64)).round() as u32;
                (num, *d)
            }
        }
    };

    // If fraction rounds to 1 or more (mixed fraction only), add to integer part
    if is_mixed && num >= denom && denom > 0 {
        let whole = num / denom;
        integer_part += whole as i64;
        num %= denom;
    }

    // Format the result
    let mut result = String::new();

    // Add sign for negative values
    if value < 0.0 {
        result.push('-');
    }

    // Format integer part (mixed fractions only)
    if is_mixed {
        if integer_part > 0 || num == 0 {
            // Format integer with digit placeholders
            let int_str = if !integer_digits.is_empty() {
                format_fraction_part(integer_part as u64, integer_digits)
            } else {
                format!("{}", integer_part)
            };
            result.push_str(&int_str);
        } else if !integer_digits.is_empty() {
            // Zero integer with non-zero fraction: show placeholders
            for placeholder in integer_digits {
                // Hash shows nothing, Question shows space, Zero shows '0'
                if let Some(c) = placeholder.empty_char() {
                    result.push(c);
                }
                // Hash returns None, so nothing is added
            }
        }
        // Add space between integer and fraction
        result.push(' ');
    }

    // Format the fraction part
    // For mixed fractions with no fractional part (num=0), use spaces instead of "0/X"
    if is_mixed && num == 0 {
        // SSF: fill(" ", 2*ri+1 + r[2].length + r[3].length)
        // This creates spaces for: numerator (ri) + slash (1) + denominator (ri) + spaces around slash
        let total_spaces = if matches!(denominator, FractionDenom::Fixed(_)) {
            // For fixed denominators, use numerator width + slash + denominator width + spaces
            let denom_width = format!("{}", denom).len();
            numerator_digits.len() + 1 + denom_width + space_before_slash.len() + space_after_slash.len()
        } else {
            2 * padding_width as usize + 1 + space_before_slash.len() + space_after_slash.len()
        };
        for _ in 0..total_spaces {
            result.push(' ');
        }
    } else {
        // Format numerator and denominator
        let num_str = format!("{}", num);
        let denom_str = format!("{}", denom);

        // Determine how to format the numerator based on fraction type
        if !integer_digits.is_empty() {
            // Mixed fraction with non-zero fractional part (e.g., "# ??/?????????" or "# ??/16")
            // SSF uses pad_(ff[1], ri) - left-pad numerator to padding_width
            let pad_width = if matches!(denominator, FractionDenom::UpToDigits(_)) {
                padding_width as usize
            } else {
                // For fixed denominators, pad to numerator placeholder width
                numerator_digits.len()
            };
            for _ in 0..pad_width.saturating_sub(num_str.len()) {
                result.push(' ');
            }
            result.push_str(&num_str);
        } else {
            // Improper fraction: use numerator_digits placeholders (e.g., "#0#00??/??")
            // SSF uses write_num("n", r[1], ff[1]) - see bits/63_numflt.js line 47
            let formatted_num = format_fraction_part(num as u64, numerator_digits);
            result.push_str(&formatted_num);
        }

        // Add spaces before slash
        result.push_str(space_before_slash);

        result.push('/');

        // Add spaces after slash
        result.push_str(space_after_slash);

        // Right-pad denominator to padding_width (for variable denominators)
        if matches!(denominator, FractionDenom::UpToDigits(_)) {
            result.push_str(&denom_str);
            for _ in 0..(padding_width as usize).saturating_sub(denom_str.len()) {
                result.push(' ');
            }
        } else {
            result.push_str(&denom_str);
        }
    }

    Ok(result)
}

/// Find the best fraction approximation for a decimal value.
/// Uses continued fractions algorithm for best rational approximation.
fn find_best_fraction(value: f64, max_denom: u32) -> (u32, u32) {
    if value == 0.0 || max_denom == 0 {
        return (0, 1);
    }

    // Handle values very close to 0
    if value.abs() < 1e-10 {
        return (0, 1);
    }

    // Use continued fractions algorithm
    let mut x = value;
    let mut a = x.floor();
    let mut h = [a as i64, 1];
    let mut k = [1_i64, 0];

    let mut n = 0;
    while n < 20 {
        // Limit iterations
        if (x - a).abs() < 1e-10 {
            break;
        }

        x = 1.0 / (x - a);
        a = x.floor();

        let h_next = a as i64 * h[0] + h[1];
        let k_next = a as i64 * k[0] + k[1];

        // Check if denominator exceeds limit
        if k_next > max_denom as i64 {
            // Return previous convergent
            break;
        }

        h[1] = h[0];
        h[0] = h_next;
        k[1] = k[0];
        k[0] = k_next;

        n += 1;
    }

    // Ensure we don't exceed max denominator
    if k[0] > max_denom as i64 {
        // Fall back to simple rounding
        let denom = max_denom.min(10);
        let num = (value * denom as f64).round() as u32;
        return (num, denom);
    }

    (h[0].max(0) as u32, k[0].max(1) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_best_fraction() {
        // Test 1/5
        let (num, denom) = find_best_fraction(0.2, 9);
        assert_eq!((num, denom), (1, 5));

        // Test 1/3
        let (num, denom) = find_best_fraction(0.333333, 9);
        assert_eq!((num, denom), (1, 3));

        // Test 2/3
        let (num, denom) = find_best_fraction(0.666666, 9);
        assert_eq!((num, denom), (2, 3));
    }
}
