//! Token types for the format code lexer.

/// A token in a format code string.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Literal(char),
    EscapedChar(char),
    QuotedString(String),

    // Digit placeholders
    Zero,     // 0
    Hash,     // #
    Question, // ?

    // Separators
    DecimalPoint, // .
    ThousandsSep, // ,
    SectionSep,   // ;

    // Special characters
    Percent,    // %
    At,         // @
    Asterisk,   // *
    Underscore, // _

    // Scientific notation
    ExponentUpper, // E
    ExponentLower, // e
    Plus,          // +
    Minus,         // -

    // Fraction
    Slash, // /

    // Date/time
    Year,   // y
    Month,  // m
    Day,    // d
    Hour,   // h
    Second, // s
    BuddhistYear,      // b (lowercase)
    BuddhistYearUpper, // B (uppercase)

    // Brackets
    OpenBracket,  // [
    CloseBracket, // ]

    // AM/PM markers
    AmPm(String), // AM/PM, am/pm, A/P, a/p

    // Special format keywords
    General, // General format keyword

    // End of input
    Eof,
}

/// A token with its position in the source.
#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub start: usize,
    pub end: usize,
}
