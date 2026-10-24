//! Number formatting (integers, decimals, percentages, scientific notation)

use crate::ast::{DigitPlaceholder, FormatPart, Section};
use crate::error::FormatError;
use crate::options::FormatOptions;

/// Format a simple integer value with digit placeholders (no separators or literals).
/// Based on SSF's write_num helper in bits/59_numhelp.js.
/// Maps digits to placeholders from right to left, using placeholder padding for missing digits.
pub(crate) fn format_simple_with_placeholders(
    value: u64,
    placeholders: &[DigitPlaceholder],
) -> String {
    if placeholders.is_empty() {
        return value.to_string();
    }

    let value_str = value.to_string();
    let value_digits: Vec<char> = value_str.chars().collect();

    // If we have more digits than placeholders, show all digits
    if value_digits.len() > placeholders.len() {
        return value_str;
    }

    // Build right-to-left into Vec, then reverse once (O(n) instead of O(n²) with insert(0))
    let mut chars = Vec::with_capacity(placeholders.len());

    // Process from right to left
    for pos_from_right in 0..placeholders.len() {
        let digit_index = value_digits.len() as isize - 1 - pos_from_right as isize;
        let placeholder_index = placeholders.len() - 1 - pos_from_right;
        let placeholder = placeholders[placeholder_index];

        if digit_index >= 0 {
            // We have a digit from the value
            chars.push(value_digits[digit_index as usize]);
        } else {
            // Use placeholder's empty character for padding
            if let Some(c) = placeholder.empty_char() {
                chars.push(c);
            }
        }
    }

    chars.reverse();
    chars.into_iter().collect()
}

/// Analysis of a format section's numeric structure.
#[derive(Debug, Clone)]
pub struct FormatAnalysis {
    /// Number of integer digit placeholders
    pub integer_placeholders: Vec<DigitPlaceholder>,
    /// Number of decimal digit placeholders
    pub decimal_placeholders: Vec<DigitPlaceholder>,
    /// Whether the format has a thousands separator
    pub has_thousands_separator: bool,
    /// Number of percent signs (each multiplies by 100)
    pub percent_count: usize,
    /// Thousands scaling factor (trailing commas divide by 1000 each)
    pub thousands_scale: usize,
    /// Literals that appear inline with integer digits (position -> literal)
    /// Position is counted from the right (0 = ones place, 1 = tens, etc.)
    pub inline_literals: Vec<(usize, String)>,
    /// Literals that appear inline with decimal digits (position -> literal)
    /// Position is counted from the left (0 = first decimal place, 1 = second, etc.)
    pub decimal_inline_literals: Vec<(usize, String)>,
    /// Parts before the number (literals, etc.)
    pub prefix_parts: Vec<FormatPart>,
    /// Parts after the number (literals, percent, etc.)
    pub suffix_parts: Vec<FormatPart>,
}

impl FormatAnalysis {
    /// Get the number of required decimal places
    pub fn decimal_places(&self) -> usize {
        self.decimal_placeholders.len()
    }

    /// Get the minimum integer digits (count of Zero placeholders)
    #[allow(dead_code)]
    pub fn min_integer_digits(&self) -> usize {
        self.integer_placeholders
            .iter()
            .filter(|p| p.is_required())
            .count()
    }
}

/// Analyze a format section to extract its numeric structure.
pub fn analyze_format(section: &Section) -> FormatAnalysis {
    let mut integer_placeholders = Vec::new();
    let mut decimal_placeholders = Vec::new();
    let mut has_thousands_separator = false;
    let mut percent_count = 0;
    let mut inline_literals = Vec::new();
    let mut decimal_inline_literals = Vec::new();
    let mut prefix_parts = Vec::new();
    let mut suffix_parts = Vec::new();

    // First, count trailing commas by scanning backwards from the end
    // Any ThousandsSeparator after the last Digit/DecimalPoint is a trailing comma
    let mut trailing_comma_count = 0;
    for part in section.parts.iter().rev() {
        match part {
            FormatPart::ThousandsSeparator => {
                trailing_comma_count += 1;
            }
            FormatPart::Digit(_) | FormatPart::DecimalPoint => {
                // Found a digit or decimal, stop counting trailing commas
                break;
            }
            _ => {
                // Other parts (Fill, Skip, Literal) - continue scanning
            }
        }
    }

    // Track which commas are trailing (to exclude from has_thousands_separator)
    let mut commas_seen = 0;
    let total_commas = section.parts.iter().filter(|p| matches!(p, FormatPart::ThousandsSeparator)).count();
    let non_trailing_comma_count = total_commas - trailing_comma_count;

    let mut seen_digit = false;
    let mut after_decimal = false;
    let mut after_digits = false;

    for part in &section.parts {
        match part {
            FormatPart::Digit(placeholder) => {
                seen_digit = true;
                after_digits = false;
                if after_decimal {
                    decimal_placeholders.push(*placeholder);
                } else {
                    integer_placeholders.push(*placeholder);
                }
            }
            FormatPart::DecimalPoint => {
                after_decimal = true;
                seen_digit = true;
                after_digits = true;  // Mark that integer digit sequence is complete
            }
            FormatPart::ThousandsSeparator => {
                commas_seen += 1;
                // Only count as thousands separator if it's not a trailing comma
                // Trailing commas are only for scaling, not for formatting separators
                if commas_seen <= non_trailing_comma_count {
                    has_thousands_separator = true;
                }
            }
            FormatPart::Percent => {
                percent_count += 1;
                if seen_digit {
                    after_digits = true;
                    suffix_parts.push(part.clone());
                } else {
                    prefix_parts.push(part.clone());
                }
            }
            FormatPart::Literal(_) | FormatPart::EscapedLiteral(_) | FormatPart::Locale(crate::ast::LocaleCode { currency: Some(_), .. }) => {
                let literal_str = if let FormatPart::Literal(s) = part {
                    s.clone()
                } else if let FormatPart::EscapedLiteral(s) = part {
                    s.clone()
                } else if let FormatPart::Locale(loc) = part {
                    loc.currency.clone().unwrap_or_default()
                } else {
                    String::new()
                };

                if !seen_digit {
                    // Before any digits - prefix
                    prefix_parts.push(part.clone());
                } else if after_digits {
                    // After all digits (after decimal or after digit sequence ended) - suffix
                    suffix_parts.push(part.clone());
                } else if after_decimal {
                    // Among decimal digits - inline literal in decimal part
                    // Store position from left (index in decimal_placeholders)
                    decimal_inline_literals.push((decimal_placeholders.len(), literal_str));
                } else {
                    // Among integer digits - inline literal
                    // Store the current placeholder count - we'll convert to position later
                    inline_literals.push((integer_placeholders.len(), literal_str));
                }
            }
            FormatPart::Locale(loc) if loc.currency.is_none() => {
                // Locale without currency - treat as before
                if !seen_digit {
                    prefix_parts.push(part.clone());
                } else if after_digits {
                    suffix_parts.push(part.clone());
                }
            }
            FormatPart::Skip(c) => {
                // Skip adds space equivalent to character width
                if !seen_digit {
                    prefix_parts.push(FormatPart::Literal(" ".to_string()));
                } else {
                    suffix_parts.push(FormatPart::Literal(" ".to_string()));
                }
                let _ = c; // suppress unused warning
            }
            _ => {
                // Handle other parts as literals in prefix/suffix
                if !seen_digit {
                    prefix_parts.push(part.clone());
                } else if after_digits {
                    suffix_parts.push(part.clone());
                }
            }
        }
    }

    // Ensure we have at least one integer placeholder for output
    if integer_placeholders.is_empty() && !after_decimal {
        integer_placeholders.push(DigitPlaceholder::Hash);
    }

    // Use the trailing comma count we calculated earlier
    let thousands_scale = trailing_comma_count;

    // Convert inline_literals from placeholder indices to positions from right
    // Inline literals are stored as (placeholder_count, string) where placeholder_count
    // is the number of placeholders added BEFORE seeing the literal.
    // This means the literal appears before placeholder at index=placeholder_count.
    // When formatting right-to-left, placeholder at index I is at position (total-1-I) from right.
    let total_placeholders = integer_placeholders.len();
    let inline_literals_converted: Vec<(usize, String)> = inline_literals
        .into_iter()
        .map(|(placeholder_count, literal)| {
            // Literal appears before placeholder[placeholder_count]
            // That placeholder is at position (total - 1 - placeholder_count) from right
            // Insert the literal AT that position (before that placeholder's digit)
            let pos_from_right = total_placeholders - placeholder_count;
            (pos_from_right, literal)
        })
        .collect();

    FormatAnalysis {
        integer_placeholders,
        decimal_placeholders,
        has_thousands_separator,
        percent_count,
        thousands_scale,
        inline_literals: inline_literals_converted,
        decimal_inline_literals,
        prefix_parts,
        suffix_parts,
    }
}

/// Format a number according to a section.
pub fn format_number(
    value: f64,
    section: &Section,
    opts: &FormatOptions,
) -> Result<String, FormatError> {
    // Check if this is scientific notation
    let scientific_part = section.parts.iter().find_map(|p| {
        if let FormatPart::Scientific { upper, show_plus } = p {
            Some((*upper, *show_plus))
        } else {
            None
        }
    });

    if let Some((upper, show_plus)) = scientific_part {
        return format_scientific(value, section, upper, show_plus, opts);
    }

    // Use pre-computed format type from metadata for better performance
    use crate::ast::FormatType;

    // Check if this is a fraction format
    if section.metadata.format_type == FormatType::Fraction {
        return crate::formatter::fraction::format_fraction(value, section, opts);
    }

    // Check if this is a text-only format
    if section.metadata.format_type == FormatType::Text {
        return Ok(crate::formatter::fallback_format(value));
    }

    // Check if section has any numeric placeholders
    let has_numeric_parts = section.metadata.format_type == FormatType::Number
        || section
            .parts
            .iter()
            .any(|p| matches!(p, FormatPart::Digit(_) | FormatPart::DecimalPoint));

    // If no numeric parts, check if GeneralNumber is present
    if !has_numeric_parts {
        let has_general_number = section
            .parts
            .iter()
            .any(|p| matches!(p, FormatPart::GeneralNumber));

        if has_general_number {
            // Section has GeneralNumber part - use General format + append literals
            // This handles cases like "General " where we want to format the number and add a suffix
            let mut result = crate::formatter::fallback_format(value);
            for part in &section.parts {
                match part {
                    FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => result.push_str(s),
                    FormatPart::Locale(locale_code) => {
                        if let Some(ref currency) = locale_code.currency {
                            result.push_str(currency);
                        }
                    }
                    FormatPart::Percent => result.push('%'),
                    FormatPart::Skip(_) => result.push(' '),
                    FormatPart::Fill(_) => {
                        // Fill character - for now just skip it
                    }
                    FormatPart::GeneralNumber => {
                        // Already handled - skip
                    }
                    _ => {}
                }
            }
            return Ok(result);
        } else {
            // No GeneralNumber - just return the literals without formatting the number
            let mut result = String::new();
            for part in &section.parts {
                match part {
                    FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => result.push_str(s),
                    FormatPart::Locale(locale_code) => {
                        if let Some(ref currency) = locale_code.currency {
                            result.push_str(currency);
                        }
                    }
                    FormatPart::Percent => result.push('%'),
                    FormatPart::Skip(_) => result.push(' '),
                    FormatPart::Fill(_) => {
                        // Fill character - for now just skip it in literal-only formats
                        // TODO: implement proper fill behavior with available width
                    }
                    _ => {}
                }
            }
            return Ok(result);
        }
    }

    let analysis = analyze_format(section);

    // Integer fast path: use integer-only arithmetic to avoid precision loss
    // Based on SSF's separate code paths in bits/66_numint.js vs bits/63_numflt.js
    // Safe integer range for f64 is < 2^53 (9007199254740992)
    //
    // Note: We only use the integer path if there are no decimal placeholders, because
    // handling optional decimal placeholders (# vs 0) requires the float path logic.
    const MAX_SAFE_INTEGER: f64 = 9007199254740992.0; // 2^53
    if value.fract() == 0.0
        && value.abs() < MAX_SAFE_INTEGER
        && analysis.decimal_placeholders.is_empty()
    {
        // Value is an exact integer within safe range and no decimal formatting needed
        return format_number_as_integer(value as i64, section, opts);
    }

    // Apply percent multiplication
    let mut adjusted_value = value.abs();
    for _ in 0..analysis.percent_count {
        adjusted_value *= 100.0;
    }

    // Apply thousands scaling (trailing commas divide by 1000 each)
    for _ in 0..analysis.thousands_scale {
        adjusted_value /= 1000.0;
    }

    // Round to the required decimal places
    // Use limited precision rounding to avoid overflow with large decimal_places
    // f64 has ~15-16 significant digits, so clamping to 15 decimal places is safe
    let decimal_places = analysis.decimal_places();
    let effective_decimal_places = decimal_places.min(15);
    let multiplier = 10_f64.powi(effective_decimal_places as i32);
    let rounded = (adjusted_value * multiplier).round() / multiplier;

    // Format the number with placeholders
    let formatted = format_with_placeholders(rounded, &analysis, opts);

    // Build the final result with prefix and suffix
    let result = build_result(&analysis, &formatted, opts);

    Ok(result)
}

/// Format an integer value using integer-only arithmetic (no precision loss).
/// Based on SSF's bits/66_numint.js.
/// This path is used for values that are exact integers within safe range (< 2^53).
fn format_number_as_integer(
    value: i64,
    section: &Section,
    opts: &FormatOptions,
) -> Result<String, FormatError> {
    let analysis = analyze_format(section);

    // Work with absolute value, track sign separately
    let mut adjusted_value = value.abs();

    // Apply percent multiplication (integer arithmetic)
    for _ in 0..analysis.percent_count {
        adjusted_value = adjusted_value.saturating_mul(100);
    }

    // Apply thousands scaling (integer division)
    for _ in 0..analysis.thousands_scale {
        adjusted_value /= 1000;
    }

    // For integers, decimal places should be zero unless explicitly formatted
    let decimal_places = analysis.decimal_places();

    if decimal_places > 0 {
        // Integer displayed with decimal places (e.g., "0.00" formatting integer 42 -> "42.00")
        // Convert to string and pad with zeros
        let integer_str = format_integer(
            adjusted_value as u64,
            &analysis.integer_placeholders,
            analysis.has_thousands_separator,
            &analysis.inline_literals,
            opts,
        );

        // Add decimal point and zeros
        let decimal_str = "0".repeat(decimal_places);
        let formatted = format!(
            "{}{}{}",
            integer_str, opts.locale.decimal_separator, decimal_str
        );

        // Build the final result with prefix and suffix
        let result = build_result(&analysis, &formatted, opts);
        Ok(result)
    } else {
        // Pure integer formatting (no decimal places)
        let formatted = format_integer(
            adjusted_value as u64,
            &analysis.integer_placeholders,
            analysis.has_thousands_separator,
            &analysis.inline_literals,
            opts,
        );

        // Build the final result with prefix and suffix
        let result = build_result(&analysis, &formatted, opts);
        Ok(result)
    }
}

/// Format a number according to the analysis.
fn format_with_placeholders(value: f64, analysis: &FormatAnalysis, opts: &FormatOptions) -> String {
    let decimal_places = analysis.decimal_places();

    // Split into integer and decimal parts
    let integer_part = value.trunc() as u64;
    let decimal_part = value.fract();

    // Format integer part
    let integer_str = format_integer(
        integer_part,
        &analysis.integer_placeholders,
        analysis.has_thousands_separator,
        &analysis.inline_literals,
        opts,
    );

    // Format decimal part
    if decimal_places > 0 {
        let decimal_str = format_decimal(
            decimal_part,
            &analysis.decimal_placeholders,
            &analysis.decimal_inline_literals,
            opts,
        );
        format!(
            "{}{}{}",
            integer_str, opts.locale.decimal_separator, decimal_str
        )
    } else {
        integer_str
    }
}

/// Format the integer part with placeholders and thousands separator.
fn format_integer(
    value: u64,
    placeholders: &[DigitPlaceholder],
    use_thousands: bool,
    inline_literals: &[(usize, String)],
    opts: &FormatOptions,
) -> String {
    let value_str = value.to_string();
    let value_digits: Vec<char> = value_str.chars().collect();

    let min_digits = placeholders.iter().filter(|p| p.is_required()).count();

    // Special case: if value is 0 and all placeholders are optional, return empty
    // BUT still include any inline literals
    if value == 0 && min_digits == 0 {
        let mut result = String::new();
        // Add any inline literals that would be in the optional placeholder region
        // Sort by position (descending) to add them left-to-right
        let mut sorted_literals: Vec<_> = inline_literals.iter().collect();
        sorted_literals.sort_by(|a, b| b.0.cmp(&a.0)); // Descending order

        for (_literal_pos, literal_str) in sorted_literals {
            // Add literals in order (left to right)
            result.push_str(literal_str);
        }
        return result;
    }

    // SSF has different logic based on whether the format includes thousands separators
    // For formats WITHOUT thousands separators (e.g., "0#######0"):
    //   - Output length = max(value_digits, total_placeholders)
    //   - Pad using "hashq" logic: 0->'0', #->skip, ?->' '
    //   - This matches SSF's bits/66_numint.js line 69-73 for ^([#0]+)\.([#0]+)$ patterns
    // For formats WITH thousands separators (e.g., "#,###"):
    //   - Use SSF's commaify approach which formats the number then adds separators
    //   - Only show digits that fit the placeholder pattern
    let output_len = if use_thousands {
        // With thousands separators: use the narrower width to avoid spurious separators
        // This matches SSF's behavior for patterns like #{1,3},##0
        value_digits.len().max(min_digits)
    } else {
        // Without thousands separators: use the full pattern width
        value_digits.len().max(placeholders.len())
    };

    // Build right-to-left into Vec, then reverse once (O(n) instead of O(n²) with insert(0))
    // Estimate capacity: output_len + separators + inline literals
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
        // Position is from the right (0 = ones place, 1 = tens, etc.)
        // Collect all literals at this position and push them
        let literals_at_pos: Vec<&str> = inline_literals
            .iter()
            .filter(|(pos, _)| *pos == pos_from_right)
            .map(|(_, s)| s.as_str())
            .collect();

        // Push in reverse order (will be reversed back at the end)
        for literal_str in literals_at_pos.iter().rev() {
            for ch in literal_str.chars().rev() {
                chars.push(ch);
            }
        }

        if digit_index >= 0 {
            // We have a digit from the value
            chars.push(value_digits[digit_index as usize]);
        } else {
            // No digit from value - apply SSF "hashq" padding logic
            // Use the placeholder at this position to determine padding character:
            //   0 (Zero) -> '0'
            //   # (Hash) -> nothing (skip)
            //   ? (Question) -> ' '
            let placeholder_index = placeholders.len() as isize - 1 - pos_from_right as isize;
            if placeholder_index >= 0 {
                let placeholder = placeholders[placeholder_index as usize];
                // empty_char returns Some('0') for Zero, None for Hash, Some(' ') for Question
                if let Some(c) = placeholder.empty_char() {
                    chars.push(c);
                }
                // If None (Hash), we don't push anything - this truncates the output
            }
        }
    }

    // Handle the case where we have no digits but need at least one
    if chars.is_empty() {
        // Check if we have any required placeholders
        if placeholders.iter().any(|p| p.is_required()) {
            chars.push('0');
        }
    }

    // Push any inline literals that are at positions beyond what we formatted
    // (literals in the leftmost optional placeholder region)
    for (literal_pos, literal_str) in inline_literals {
        if *literal_pos >= output_len {
            // This literal is to the left of all displayed digits
            for ch in literal_str.chars().rev() {
                chars.push(ch);
            }
        }
    }

    // Reverse once and collect into String
    chars.reverse();
    let result: String = chars.into_iter().collect();

    result
}

/// Format the decimal part with placeholders.
fn format_decimal(
    value: f64,
    placeholders: &[DigitPlaceholder],
    decimal_inline_literals: &[(usize, String)],
    _opts: &FormatOptions,
) -> String {
    if placeholders.is_empty() {
        return String::new();
    }

    // Match SSF behavior: clamp decimal places to 10 (bits/66_numint.js line 70)
    // This avoids floating-point precision issues when multiplying by large powers of 10
    // SSF uses Math.min(r[2].length, 10) where r[2] is the decimal placeholder count
    let effective_places = placeholders.len().min(10);

    // Get the decimal digits by multiplying and truncating
    let multiplier = 10_f64.powi(effective_places as i32);
    let decimal_int = (value * multiplier).round() as u64;
    let decimal_str = format!("{:0>width$}", decimal_int, width = effective_places);
    let decimal_chars: Vec<char> = decimal_str.chars().collect();

    let mut result = String::new();

    // Check if the entire decimal part is zeros (matches SSF behavior)
    // SSF strips all trailing zeros with regex /([^0])0+$/ before applying format
    let all_zeros = decimal_chars.iter().all(|&c| c == '0');

    // Find where trailing zeros start (for # placeholders)
    // If all zeros, start from position 0 (all Hash placeholders are skipped)
    // Otherwise, scan backwards to find trailing zeros
    let mut trailing_zeros_start = if all_zeros {
        0
    } else {
        placeholders.len()
    };

    // Only scan within effective_places to avoid index out of bounds
    if !all_zeros {
        for i in (0..placeholders.len().min(effective_places)).rev() {
            if decimal_chars.get(i) == Some(&'0') {
                if !placeholders[i].is_required() {
                    trailing_zeros_start = i;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Build result, respecting placeholder rules
    for (i, placeholder) in placeholders.iter().enumerate() {
        // Insert any decimal inline literals that appear at this position
        for (literal_pos, literal_str) in decimal_inline_literals {
            if *literal_pos == i {
                result.push_str(literal_str);
            }
        }

        // Get the character for this position
        let ch = if i < effective_places {
            decimal_chars.get(i).copied().unwrap_or('0')
        } else {
            // Beyond effective precision: apply SSF "hashq" logic
            // Hash (#) -> skip (no output)
            // Zero (0) -> '0'
            // Question (?) -> ' '
            match placeholder {
                DigitPlaceholder::Hash => {
                    // Skip - don't add anything
                    continue;
                }
                DigitPlaceholder::Zero => '0',
                DigitPlaceholder::Question => ' ',
            }
        };

        if i >= trailing_zeros_start && ch == '0' && !placeholder.is_required() {
            // Skip trailing zeros for # placeholders (only within effective_places)
            if matches!(placeholder, DigitPlaceholder::Question) {
                result.push(' ');
            }
            // For Hash, we don't add anything
        } else {
            result.push(ch);
        }
    }

    // Append any decimal inline literals that come after all placeholders
    for (literal_pos, literal_str) in decimal_inline_literals {
        if *literal_pos >= placeholders.len() {
            result.push_str(literal_str);
        }
    }

    result
}

/// Calculate the exact character count for format parts (prefix/suffix).
fn count_part_chars(parts: &[FormatPart]) -> usize {
    parts.iter().map(|part| {
        match part {
            FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => s.len(),
            FormatPart::Locale(locale_code) => {
                locale_code.currency.as_ref().map_or(0, |s| s.len())
            }
            FormatPart::Percent => 1,
            _ => 0,
        }
    }).sum()
}

/// Build the final result string with prefix and suffix parts.
fn build_result(
    analysis: &FormatAnalysis,
    formatted_number: &str,
    _opts: &FormatOptions,
) -> String {
    // Pre-allocate exact capacity (no reallocation, no waste)
    let capacity = count_part_chars(&analysis.prefix_parts)
        + formatted_number.len()
        + count_part_chars(&analysis.suffix_parts);
    let mut result = String::with_capacity(capacity);

    // Add prefix parts
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
    result.push_str(formatted_number);

    // Add suffix parts
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

    result
}

/// Format a number in scientific notation according to a format section.
fn format_scientific(
    value: f64,
    section: &Section,
    upper: bool,
    show_plus: bool,
    _opts: &FormatOptions,
) -> Result<String, FormatError> {
    // Count digits before and after decimal in mantissa, and exponent digits
    let mut mantissa_integer_places = 0;
    let mut mantissa_decimal_places = 0;
    let mut exponent_digits = 0;
    let mut seen_decimal = false;
    let mut after_exponent = false;

    for part in &section.parts {
        match part {
            FormatPart::Digit(_) if !seen_decimal && !after_exponent => {
                mantissa_integer_places += 1;
            }
            FormatPart::DecimalPoint if !after_exponent => {
                seen_decimal = true;
            }
            FormatPart::Digit(_) if seen_decimal && !after_exponent => {
                mantissa_decimal_places += 1;
            }
            FormatPart::Scientific { .. } => {
                after_exponent = true;
            }
            FormatPart::Digit(_) if after_exponent => {
                exponent_digits += 1;
            }
            _ => {}
        }
    }

    // Convert value to scientific notation
    let abs_value = value.abs();

    // Handle zero specially
    if abs_value == 0.0 {
        let zeros = "0".repeat(mantissa_decimal_places);
        let decimal_part = if mantissa_decimal_places > 0 {
            format!(".{}", zeros)
        } else {
            String::new()
        };
        let exp_char = if upper { 'E' } else { 'e' };
        let sign = if show_plus { "+" } else { "" };
        return Ok(format!("0{}{}{sign}00", decimal_part, exp_char));
    }

    // Calculate exponent based on integer placeholder count
    // Standard format (0) or minimal format (no placeholder): mantissa 1-10, exponent = log10(value)
    // Format with multiple placeholders (##0): adjust exponent to use more mantissa digits
    let base_exponent = abs_value.log10().floor() as i32;

    let exponent = if mantissa_integer_places > 1 {
        // For ##0 (3 places), we want mantissa to be in range [1, 1000)
        // Adjust exponent to be a multiple of group_size to group digits
        // For ##0: exponent should be multiple of 3, giving mantissa like 123.5E+6, not 1.235E+8
        let group_size = mantissa_integer_places.max(1);
        // Use floor division to handle negative exponents correctly
        // For base_exponent = -1, group_size = 3: floor(-1/3) * 3 = -1 * 3 = -3
        ((base_exponent as f64) / (group_size as f64)).floor() as i32 * group_size
    } else {
        base_exponent
    };

    let mantissa = abs_value / 10_f64.powi(exponent);

    // Format mantissa with appropriate decimal places
    let mantissa_str = if mantissa_decimal_places > 0 {
        format!("{:.prec$}", mantissa, prec = mantissa_decimal_places)
    } else {
        format!("{:.0}", mantissa)
    };

    // Format exponent
    let exp_char = if upper { 'E' } else { 'e' };
    let exp_sign = if exponent >= 0 {
        if show_plus { "+" } else { "" }
    } else {
        "-"
    };
    let exp_abs = exponent.abs();

    // Format exponent with appropriate zero padding
    let exp_str = if exponent_digits >= 2 {
        // 0.00E+00 format uses 2-digit exponents
        format!("{:02}", exp_abs)
    } else {
        // ##0.0E+0 format uses minimal digits
        format!("{}", exp_abs)
    };
    let formatted = format!("{}{}{}{}", mantissa_str, exp_char, exp_sign, exp_str);

    // Apply sign for negative values
    if value < 0.0 {
        Ok(format!("-{}", formatted))
    } else {
        Ok(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Section;

    fn make_section(parts: Vec<FormatPart>) -> Section {
        Section {
            condition: None,
            color: None,
            parts,
            metadata: crate::ast::SectionMetadata::default(),
        }
    }

    #[test]
    fn test_analyze_simple_integer() {
        let section = make_section(vec![FormatPart::Digit(DigitPlaceholder::Zero)]);
        let analysis = analyze_format(&section);

        assert_eq!(analysis.integer_placeholders.len(), 1);
        assert_eq!(analysis.decimal_placeholders.len(), 0);
        assert!(!analysis.has_thousands_separator);
        assert_eq!(analysis.percent_count, 0);
    }

    #[test]
    fn test_analyze_decimal_format() {
        let section = make_section(vec![
            FormatPart::Digit(DigitPlaceholder::Zero),
            FormatPart::DecimalPoint,
            FormatPart::Digit(DigitPlaceholder::Zero),
            FormatPart::Digit(DigitPlaceholder::Zero),
        ]);
        let analysis = analyze_format(&section);

        assert_eq!(analysis.integer_placeholders.len(), 1);
        assert_eq!(analysis.decimal_placeholders.len(), 2);
    }

    #[test]
    fn test_analyze_thousands() {
        let section = make_section(vec![
            FormatPart::Digit(DigitPlaceholder::Hash),
            FormatPart::ThousandsSeparator,
            FormatPart::Digit(DigitPlaceholder::Hash),
            FormatPart::Digit(DigitPlaceholder::Hash),
            FormatPart::Digit(DigitPlaceholder::Zero),
        ]);
        let analysis = analyze_format(&section);

        assert!(analysis.has_thousands_separator);
        assert_eq!(analysis.integer_placeholders.len(), 4);
    }

    #[test]
    fn test_analyze_percent() {
        let section = make_section(vec![
            FormatPart::Digit(DigitPlaceholder::Zero),
            FormatPart::Percent,
        ]);
        let analysis = analyze_format(&section);

        assert_eq!(analysis.percent_count, 1);
        assert_eq!(analysis.suffix_parts.len(), 1);
    }
}
