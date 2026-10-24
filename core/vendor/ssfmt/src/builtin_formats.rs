//! Built-in number format IDs and their corresponding format codes.
//!
//! Excel uses numeric format IDs (0-49 and others) for built-in formats.
//! These IDs are stored in .xlsx files but the actual format codes are implied.
//! This module provides the mapping from format IDs to format code strings.
//!
//! Based on ECMA-376 and Excel's actual implementation, matching the behavior
//! from SheetJS's ssf library.

/// Get the format code string for a built-in format ID.
///
/// Excel stores format IDs in .xlsx files (numFmtId attribute), but the actual
/// format codes are implied for IDs in the range 0-49. Custom formats start at 164.
///
/// # Arguments
/// * `id` - The numeric format ID from the spreadsheet file
///
/// # Returns
/// * `Some(format_code)` - The format code string if this is a built-in format
/// * `None` - If the ID is not a recognized built-in format
///
/// # Examples
/// ```
/// use ssfmt::format_code_from_id;
///
/// assert_eq!(format_code_from_id(0), Some("General"));
/// assert_eq!(format_code_from_id(1), Some("0"));
/// assert_eq!(format_code_from_id(14), Some("m/d/yy"));
/// assert_eq!(format_code_from_id(164), None); // Custom format
/// ```
pub fn format_code_from_id(id: u32) -> Option<&'static str> {
    match id {
        0 => Some("General"),
        1 => Some("0"),
        2 => Some("0.00"),
        3 => Some("#,##0"),
        4 => Some("#,##0.00"),
        9 => Some("0%"),
        10 => Some("0.00%"),
        11 => Some("0.00E+00"),
        12 => Some("# ?/?"),
        13 => Some("# ??/??"),
        14 => Some("m/d/yy"), // Excel uses this, not spec's "mm-dd-yy"
        15 => Some("d-mmm-yy"),
        16 => Some("d-mmm"),
        17 => Some("mmm-yy"),
        18 => Some("h:mm AM/PM"),
        19 => Some("h:mm:ss AM/PM"),
        20 => Some("h:mm"),
        21 => Some("h:mm:ss"),
        22 => Some("m/d/yy h:mm"),
        37 => Some("#,##0 ;(#,##0)"),
        38 => Some("#,##0 ;[Red](#,##0)"),
        39 => Some("#,##0.00;(#,##0.00)"),
        40 => Some("#,##0.00;[Red](#,##0.00)"),
        45 => Some("mm:ss"),
        46 => Some("[h]:mm:ss"),
        47 => Some("mmss.0"),
        48 => Some("##0.0E+0"),
        49 => Some("@"),
        // Note: IDs 5-8, 23-36, 41-44, 50+ are not defined as built-in formats
        // Custom formats typically start at 164
        _ => None,
    }
}

/// Check if a format ID is a built-in format.
///
/// Built-in formats are those in the range 0-49 that have predefined format codes.
/// Custom formats typically start at 164.
///
/// # Examples
/// ```
/// use ssfmt::is_builtin_format_id;
///
/// assert!(is_builtin_format_id(0));
/// assert!(is_builtin_format_id(14));
/// assert!(!is_builtin_format_id(164));
/// ```
pub fn is_builtin_format_id(id: u32) -> bool {
    format_code_from_id(id).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_general_format() {
        assert_eq!(format_code_from_id(0), Some("General"));
    }

    #[test]
    fn test_basic_number_formats() {
        assert_eq!(format_code_from_id(1), Some("0"));
        assert_eq!(format_code_from_id(2), Some("0.00"));
        assert_eq!(format_code_from_id(3), Some("#,##0"));
        assert_eq!(format_code_from_id(4), Some("#,##0.00"));
    }

    #[test]
    fn test_percentage_formats() {
        assert_eq!(format_code_from_id(9), Some("0%"));
        assert_eq!(format_code_from_id(10), Some("0.00%"));
    }

    #[test]
    fn test_date_formats() {
        assert_eq!(format_code_from_id(14), Some("m/d/yy"));
        assert_eq!(format_code_from_id(15), Some("d-mmm-yy"));
        assert_eq!(format_code_from_id(22), Some("m/d/yy h:mm"));
    }

    #[test]
    fn test_time_formats() {
        assert_eq!(format_code_from_id(18), Some("h:mm AM/PM"));
        assert_eq!(format_code_from_id(20), Some("h:mm"));
        assert_eq!(format_code_from_id(21), Some("h:mm:ss"));
        assert_eq!(format_code_from_id(46), Some("[h]:mm:ss"));
    }

    #[test]
    fn test_scientific_and_fraction() {
        assert_eq!(format_code_from_id(11), Some("0.00E+00"));
        assert_eq!(format_code_from_id(12), Some("# ?/?"));
        assert_eq!(format_code_from_id(48), Some("##0.0E+0"));
    }

    #[test]
    fn test_accounting_formats() {
        assert_eq!(format_code_from_id(37), Some("#,##0 ;(#,##0)"));
        assert_eq!(format_code_from_id(38), Some("#,##0 ;[Red](#,##0)"));
        assert_eq!(format_code_from_id(39), Some("#,##0.00;(#,##0.00)"));
        assert_eq!(format_code_from_id(40), Some("#,##0.00;[Red](#,##0.00)"));
    }

    #[test]
    fn test_text_format() {
        assert_eq!(format_code_from_id(49), Some("@"));
    }

    #[test]
    fn test_undefined_ids() {
        // IDs that are not defined as built-in formats
        assert_eq!(format_code_from_id(5), None);
        assert_eq!(format_code_from_id(6), None);
        assert_eq!(format_code_from_id(7), None);
        assert_eq!(format_code_from_id(8), None);
        assert_eq!(format_code_from_id(23), None);
        assert_eq!(format_code_from_id(50), None);
        assert_eq!(format_code_from_id(164), None); // Custom format
    }

    #[test]
    fn test_is_builtin() {
        assert!(is_builtin_format_id(0));
        assert!(is_builtin_format_id(14));
        assert!(is_builtin_format_id(49));
        assert!(!is_builtin_format_id(5));
        assert!(!is_builtin_format_id(164));
    }
}
