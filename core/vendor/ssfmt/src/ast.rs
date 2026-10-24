//! AST types for parsed format codes.

use crate::error::ParseError;
use std::str::FromStr;

/// Named colors supported in format codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedColor {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    White,
    Yellow,
}

impl FromStr for NamedColor {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "black" => Ok(NamedColor::Black),
            "blue" => Ok(NamedColor::Blue),
            "cyan" => Ok(NamedColor::Cyan),
            "green" => Ok(NamedColor::Green),
            "magenta" => Ok(NamedColor::Magenta),
            "red" => Ok(NamedColor::Red),
            "white" => Ok(NamedColor::White),
            "yellow" => Ok(NamedColor::Yellow),
            _ => Err(()),
        }
    }
}

/// Color specification in a format section.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Named(NamedColor),
    Indexed(u8),
}

/// Conditional expression for section selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Condition {
    GreaterThan(f64),
    LessThan(f64),
    Equal(f64),
    GreaterOrEqual(f64),
    LessOrEqual(f64),
    NotEqual(f64),
}

impl Condition {
    /// Evaluate this condition against a value.
    pub fn evaluate(&self, value: f64) -> bool {
        match self {
            Condition::GreaterThan(n) => value > *n,
            Condition::LessThan(n) => value < *n,
            Condition::Equal(n) => (value - n).abs() < f64::EPSILON,
            Condition::GreaterOrEqual(n) => value >= *n,
            Condition::LessOrEqual(n) => value <= *n,
            Condition::NotEqual(n) => (value - n).abs() >= f64::EPSILON,
        }
    }

    /// Check if value strictly satisfies the condition (not at boundary).
    /// For <=, >=, and = conditions, returns true only if not exactly equal.
    /// For <, >, and != conditions, returns same as evaluate().
    pub fn is_strict_match(&self, value: f64) -> bool {
        match self {
            Condition::GreaterThan(n) => value > *n,
            Condition::LessThan(n) => value < *n,
            Condition::Equal(n) => (value - n).abs() < f64::EPSILON,
            Condition::GreaterOrEqual(n) => value > *n, // Strictly greater, not equal
            Condition::LessOrEqual(n) => value < *n,    // Strictly less, not equal
            Condition::NotEqual(n) => (value - n).abs() >= f64::EPSILON,
        }
    }
}

/// Digit placeholder type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigitPlaceholder {
    /// `0` - Display digit or zero
    Zero,
    /// `#` - Display digit or nothing
    Hash,
    /// `?` - Display digit or space
    Question,
}

impl DigitPlaceholder {
    /// Returns true if this placeholder requires a digit (shows 0 for missing).
    pub fn is_required(&self) -> bool {
        matches!(self, DigitPlaceholder::Zero)
    }

    /// Returns the character to display when no digit is present.
    pub fn empty_char(&self) -> Option<char> {
        match self {
            DigitPlaceholder::Zero => Some('0'),
            DigitPlaceholder::Hash => None,
            DigitPlaceholder::Question => Some(' '),
        }
    }
}

/// Date/time format parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePart {
    /// `yy` - Two-digit year
    Year2,
    /// `yyy` - Three-digit year (minimum 3 digits, shows full year)
    Year3,
    /// `yyyy` - Four-digit year
    Year4,
    /// `m` - Month as number without leading zero (1-12)
    Month,
    /// `mm` - Month as number with leading zero (01-12)
    Month2,
    /// `mmm` - Month as abbreviated name (Jan, Feb, etc.)
    MonthAbbr,
    /// `mmmm` - Month as full name (January, February, etc.)
    MonthFull,
    /// `mmmmm` - Month as single letter (J, F, M, etc.)
    MonthLetter,
    /// `d` - Day of month without leading zero (1-31)
    Day,
    /// `dd` - Day of month with leading zero (01-31)
    Day2,
    /// `ddd` - Day of week as abbreviated name (Sun, Mon, etc.)
    DayAbbr,
    /// `dddd` - Day of week as full name (Sunday, Monday, etc.)
    DayFull,
    /// `h` - Hour without leading zero (0-23 or 1-12 with AM/PM)
    Hour,
    /// `hh` - Hour with leading zero (00-23 or 01-12 with AM/PM)
    Hour2,
    /// `m` - Minute without leading zero (0-59), when following h/hh
    Minute,
    /// `mm` - Minute with leading zero (00-59), when following h/hh
    Minute2,
    /// `s` - Second without leading zero (0-59)
    Second,
    /// `ss` - Second with leading zero (00-59)
    Second2,
    /// `.0`, `.00`, etc. - Fractional seconds with specified decimal places
    SubSecond(u8),
    /// `b` or `bb` - Buddhist year (Thai calendar), last 2 digits (Gregorian + 543)
    BuddhistYear2,
    /// `bbbb` - Buddhist year (Thai calendar), 4 digits (Gregorian + 543)
    BuddhistYear4,
    /// `B2yyyy` - Alternative Buddhist calendar era, 4 digits (Gregorian - 582)
    BuddhistYear4Alt,
    /// `B2yy` - Alternative Buddhist calendar era, last 2 digits (Gregorian - 582)
    BuddhistYear2Alt,
}

/// AM/PM format style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmPmStyle {
    /// `AM/PM` - Uppercase AM or PM
    Upper,
    /// `am/pm` - Lowercase am or pm
    Lower,
    /// `A/P` - Uppercase single letter A or P
    ShortUpper,
    /// `a/p` - Lowercase single letter a or p
    ShortLower,
    /// `AM/P` - Malformed uppercase pattern (outputs A0/P or A1/P)
    MalformedUpper,
    /// `am/p` - Malformed lowercase pattern (outputs a0/p or a1/p)
    MalformedLower,
}

/// Elapsed time format part (for durations).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElapsedPart {
    /// `[h]` - Total elapsed hours without padding
    Hours,
    /// `[hh]` - Total elapsed hours with zero-padding to 2 digits
    Hours2,
    /// `[m]` - Total elapsed minutes without padding
    Minutes,
    /// `[mm]` - Total elapsed minutes with zero-padding to 2 digits
    Minutes2,
    /// `[s]` - Total elapsed seconds without padding
    Seconds,
    /// `[ss]` - Total elapsed seconds with zero-padding to 2 digits
    Seconds2,
}

/// Fraction denominator specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractionDenom {
    UpToDigits(u8),
    Fixed(u32),
}

/// Locale code from format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleCode {
    /// Currency symbol to display (e.g., "$", "€", "£")
    pub currency: Option<String>,
    /// Windows Locale Identifier for language/region-specific formatting
    pub lcid: Option<u32>,
}

/// A single part of a format section.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatPart {
    /// Literal text to display as-is (from unescaped characters or quoted strings)
    Literal(String),
    /// Escaped literal character (e.g., `\r`, `\#`) - does not get minus sign for negative values
    EscapedLiteral(String),
    /// Digit placeholder (0, #, or ?)
    Digit(DigitPlaceholder),
    /// Decimal point separator
    DecimalPoint,
    /// Thousands grouping separator (comma in US locale)
    ThousandsSeparator,
    /// Percent sign - multiplies value by 100
    Percent,
    /// Scientific notation (E+, E-, e+, e-)
    Scientific {
        /// True for uppercase E, false for lowercase e
        upper: bool,
        /// True to always show sign, false for minus only
        show_plus: bool,
    },
    /// Fraction format (e.g., # #/# or # ??/??)
    Fraction {
        /// Digit placeholders for integer part
        integer_digits: Vec<DigitPlaceholder>,
        /// Digit placeholders for numerator
        numerator_digits: Vec<DigitPlaceholder>,
        /// Denominator specification (fixed or up to N digits)
        denominator: FractionDenom,
        /// Space before slash (for formats like "# ?? / ??")
        space_before_slash: String,
        /// Space after slash (for formats like "# ?? / ??")
        space_after_slash: String,
    },
    /// Date/time component
    DatePart(DatePart),
    /// AM/PM indicator
    AmPm(AmPmStyle),
    /// Elapsed time component for durations
    Elapsed(ElapsedPart),
    /// `@` - Text placeholder for text values
    TextPlaceholder,
    /// `*x` - Repeat character to fill available width
    Fill(char),
    /// `_x` - Skip width of character (for alignment)
    Skip(char),
    /// `[$...]` - Locale/currency specification
    Locale(LocaleCode),
    /// General number formatting (used when "General" keyword appears with additional format parts)
    GeneralNumber,
}

impl FormatPart {
    /// Returns true if this is a date/time part.
    pub fn is_date_part(&self) -> bool {
        matches!(
            self,
            FormatPart::DatePart(_) | FormatPart::AmPm(_) | FormatPart::Elapsed(_)
        )
    }

    /// Returns true if this is a numeric formatting part.
    pub fn is_numeric_part(&self) -> bool {
        matches!(
            self,
            FormatPart::Digit(_)
                | FormatPart::DecimalPoint
                | FormatPart::ThousandsSeparator
                | FormatPart::Percent
                | FormatPart::Scientific { .. }
                | FormatPart::Fraction { .. }
        )
    }
}

/// Smallest time unit displayed in a format (used for pre-rounding).
/// Based on SSF's `bt` variable in bits/82_eval.js
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeUnit {
    /// No time components in format
    None,
    /// Hours is the smallest unit (round to nearest minute)
    Hours,
    /// Minutes is the smallest unit (round to nearest second)
    Minutes,
    /// Seconds is the smallest unit (round to nearest subsecond)
    Seconds,
    /// Subseconds displayed (no rounding needed)
    Subseconds,
}

/// Type of format for optimization and dispatch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatType {
    /// General number format or mixed
    General,
    /// Date/time format
    DateTime,
    /// Pure number format
    Number,
    /// Fraction format
    Fraction,
    /// Text format
    Text,
}

/// Pre-computed metadata about a section to avoid repeated scanning
#[derive(Debug, Clone, PartialEq)]
pub struct SectionMetadata {
    /// True if format contains AM/PM indicator
    pub has_ampm: bool,
    /// True if format uses Hijri calendar (B2 prefix)
    pub is_hijri: bool,
    /// Maximum subsecond precision (e.g., 3 for .000)
    pub max_subsecond_precision: Option<u8>,
    /// True if format contains elapsed time components ([h], [m], [s])
    pub has_elapsed_time: bool,
    /// Smallest time unit displayed (for pre-rounding)
    pub smallest_time_unit: TimeUnit,
    /// Primary format type
    pub format_type: FormatType,
}

impl Default for SectionMetadata {
    fn default() -> Self {
        Self {
            has_ampm: false,
            is_hijri: false,
            max_subsecond_precision: None,
            has_elapsed_time: false,
            smallest_time_unit: TimeUnit::None,
            format_type: FormatType::General,
        }
    }
}

/// A single section of a format code.
///
/// Format codes can have up to 4 sections:
/// 1. Positive numbers (or all numbers if only one section)
/// 2. Negative numbers
/// 3. Zero
/// 4. Text
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// Optional condition for this section (e.g., [>100])
    pub condition: Option<Condition>,
    /// Optional color for this section (e.g., [Red])
    pub color: Option<Color>,
    /// The format parts that make up this section
    pub parts: Vec<FormatPart>,
    /// Pre-computed metadata to avoid repeated scanning
    pub metadata: SectionMetadata,
}

impl Section {
    /// Returns true if this section contains any date/time parts.
    pub fn has_date_parts(&self) -> bool {
        self.parts.iter().any(|p| p.is_date_part())
    }

    /// Returns true if this section contains a text placeholder.
    pub fn has_text_placeholder(&self) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, FormatPart::TextPlaceholder))
    }

    /// Returns true if this section contains a percent sign.
    pub fn has_percent(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, FormatPart::Percent))
    }
}

/// A parsed number format code.
///
/// This is the main type returned by parsing. It can be reused to format
/// multiple values efficiently.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberFormat {
    sections: Vec<Section>,
}

impl NumberFormat {
    /// Create a NumberFormat from parsed sections.
    /// Limits to 4 sections maximum per Excel spec.
    pub fn from_sections(sections: Vec<Section>) -> Self {
        let sections = if sections.len() > 4 {
            sections.into_iter().take(4).collect()
        } else {
            sections
        };
        NumberFormat { sections }
    }

    /// Get the sections of this format.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Returns true if this format contains date/time parts.
    pub fn is_date_format(&self) -> bool {
        self.sections.iter().any(|s| s.has_date_parts())
    }

    /// Returns true if this is a text-only format.
    pub fn is_text_format(&self) -> bool {
        self.sections.len() == 1 && self.sections[0].has_text_placeholder()
    }

    /// Returns true if this format contains a percent sign.
    pub fn is_percentage(&self) -> bool {
        self.sections.iter().any(|s| s.has_percent())
    }

    /// Returns true if any section has a color.
    pub fn has_color(&self) -> bool {
        self.sections.iter().any(|s| s.color.is_some())
    }

    /// Returns true if any section has a condition.
    pub fn has_condition(&self) -> bool {
        self.sections.iter().any(|s| s.condition.is_some())
    }

    /// Parse a format code string into a NumberFormat.
    pub fn parse(format_code: &str) -> Result<NumberFormat, ParseError> {
        crate::parser::parse(format_code)
    }
}
