//! Lexer for tokenizing format code strings.
//!
//! The lexer converts format code strings into a stream of tokens that can be
//! processed by the parser. It handles special cases like:
//! - Date/time letters (y, m, d, h, s) are tokens outside brackets but literals inside brackets
//! - Quoted strings ("text") become QuotedString tokens
//! - Escaped characters (\$) become EscapedChar tokens
//! - AM/PM patterns are detected and returned as single tokens

use crate::error::ParseError;
use crate::parser::tokens::{SpannedToken, Token};

/// Run type constants for pending run tracking.
const RUN_YEAR: u8 = 0;
const RUN_MONTH: u8 = 1;
const RUN_DAY: u8 = 2;
const RUN_HOUR: u8 = 3;
const RUN_SECOND: u8 = 4;
const RUN_ZERO: u8 = 5;
const RUN_HASH: u8 = 6;
const RUN_QUESTION: u8 = 7;

/// A lexer for format code strings.
pub struct Lexer<'a> {
    /// The input string being tokenized.
    pub(crate) input: &'a str,
    /// The current position in the input.
    position: usize,
    /// Whether we are currently inside brackets.
    in_bracket: bool,
    /// Pending run of tokens: (run_type, remaining_count, next_token_pos)
    /// When we encounter consecutive same-type chars (e.g., "yyyy"),
    /// we count them once and emit tokens from this counter.
    pending_run: Option<(u8, usize, usize)>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given input string.
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            in_bracket: false,
            pending_run: None,
        }
    }

    /// Returns the next token from the input.
    pub fn next_token(&mut self) -> Result<SpannedToken, ParseError> {
        // First, check if we have pending tokens from a run
        if let Some((run_type, remaining, next_pos)) = self.pending_run {
            let token = match run_type {
                RUN_YEAR => Token::Year,
                RUN_MONTH => Token::Month,
                RUN_DAY => Token::Day,
                RUN_HOUR => Token::Hour,
                RUN_SECOND => Token::Second,
                RUN_ZERO => Token::Zero,
                RUN_HASH => Token::Hash,
                RUN_QUESTION => Token::Question,
                _ => unreachable!(),
            };
            if remaining <= 1 {
                self.pending_run = None;
            } else {
                self.pending_run = Some((run_type, remaining - 1, next_pos + 1));
            }
            return Ok(SpannedToken {
                token,
                start: next_pos,
                end: next_pos + 1,
            });
        }

        if self.position >= self.input.len() {
            return Ok(SpannedToken {
                token: Token::Eof,
                start: self.position,
                end: self.position,
            });
        }

        let start = self.position;
        let ch = self.current_char().unwrap();

        // Try to match special keywords first (before consuming individual characters)
        // Only check if current character could start the pattern (avoid unnecessary work)
        if !self.in_bracket {
            // Try to match "General" keyword (only if starts with 'G' or 'g')
            if ch == 'G' || ch == 'g' {
                if let Some(general_token) = self.try_match_general() {
                    return Ok(general_token);
                }
            }

            // Try to match AM/PM patterns (only if starts with 'A' or 'a')
            if ch == 'A' || ch == 'a' {
                if let Some(am_pm_token) = self.try_match_am_pm() {
                    return Ok(am_pm_token);
                }
            }
        }

        let token = match ch {
            // Quoted string
            '"' => self.lex_quoted_string()?,

            // Escaped character
            '\\' => self.lex_escaped_char()?,

            // Digit placeholders - batch consecutive runs
            '0' => {
                let count = self.count_run(|c| c == '0');
                if count > 1 {
                    // next token position is start + 1
                    self.pending_run = Some((RUN_ZERO, count - 1, start + 1));
                }
                Token::Zero
            }
            '#' => {
                let count = self.count_run(|c| c == '#');
                if count > 1 {
                    self.pending_run = Some((RUN_HASH, count - 1, start + 1));
                }
                Token::Hash
            }
            '?' => {
                let count = self.count_run(|c| c == '?');
                if count > 1 {
                    self.pending_run = Some((RUN_QUESTION, count - 1, start + 1));
                }
                Token::Question
            }

            // Separators
            '.' => {
                self.advance();
                Token::DecimalPoint
            }
            ',' => {
                self.advance();
                Token::ThousandsSep
            }
            ';' => {
                self.advance();
                Token::SectionSep
            }

            // Special characters
            '%' => {
                self.advance();
                Token::Percent
            }
            '@' => {
                self.advance();
                Token::At
            }
            '*' => {
                self.advance();
                Token::Asterisk
            }
            '_' => {
                self.advance();
                Token::Underscore
            }

            // Scientific notation
            'E' => {
                self.advance();
                Token::ExponentUpper
            }
            'e' if !self.in_bracket => {
                self.advance();
                Token::ExponentLower
            }

            // Signs
            '+' => {
                self.advance();
                Token::Plus
            }
            '-' => {
                self.advance();
                Token::Minus
            }

            // Fraction
            '/' => {
                self.advance();
                Token::Slash
            }

            // Brackets
            '[' => {
                self.advance();
                self.in_bracket = true;
                Token::OpenBracket
            }
            ']' => {
                self.advance();
                self.in_bracket = false;
                Token::CloseBracket
            }

            // Date/time characters (only outside brackets) - batch consecutive runs
            'y' | 'Y' if !self.in_bracket => {
                let count = self.count_run(|c| c == 'y' || c == 'Y');
                if count > 1 {
                    self.pending_run = Some((RUN_YEAR, count - 1, start + 1));
                }
                Token::Year
            }
            'm' | 'M' if !self.in_bracket => {
                let count = self.count_run(|c| c == 'm' || c == 'M');
                if count > 1 {
                    self.pending_run = Some((RUN_MONTH, count - 1, start + 1));
                }
                Token::Month
            }
            'd' | 'D' if !self.in_bracket => {
                let count = self.count_run(|c| c == 'd' || c == 'D');
                if count > 1 {
                    self.pending_run = Some((RUN_DAY, count - 1, start + 1));
                }
                Token::Day
            }
            'h' | 'H' if !self.in_bracket => {
                let count = self.count_run(|c| c == 'h' || c == 'H');
                if count > 1 {
                    self.pending_run = Some((RUN_HOUR, count - 1, start + 1));
                }
                Token::Hour
            }
            's' | 'S' if !self.in_bracket => {
                let count = self.count_run(|c| c == 's' || c == 'S');
                if count > 1 {
                    self.pending_run = Some((RUN_SECOND, count - 1, start + 1));
                }
                Token::Second
            }
            'b' if !self.in_bracket => {
                self.advance();
                Token::BuddhistYear
            }
            'B' if !self.in_bracket => {
                self.advance();
                Token::BuddhistYearUpper
            }

            // Everything else is a literal
            _ => {
                self.advance();
                Token::Literal(ch)
            }
        };

        // For runs, we've consumed all consecutive chars but return one token at a time.
        // The end position should be start + 1 for the first token of a run.
        let end = if self.pending_run.is_some() {
            start + 1
        } else {
            self.position
        };

        Ok(SpannedToken {
            token,
            start,
            end,
        })
    }

    /// Returns the character at the current position, if any.
    fn current_char(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    /// Returns the remaining input as a string slice.
    fn remaining(&self) -> &str {
        &self.input[self.position..]
    }

    /// Advances the position by one character.
    fn advance(&mut self) {
        if let Some(ch) = self.current_char() {
            self.position += ch.len_utf8();
        }
    }

    /// Counts and consumes consecutive characters matching the predicate.
    /// Returns the count (always >= 1 since current char matches).
    #[inline]
    fn count_run<F>(&mut self, predicate: F) -> usize
    where
        F: Fn(char) -> bool,
    {
        let mut count = 0;
        while let Some(ch) = self.current_char() {
            if predicate(ch) {
                count += 1;
                self.advance();
            } else {
                break;
            }
        }
        count
    }

    /// Lexes a quoted string ("...").
    fn lex_quoted_string(&mut self) -> Result<Token, ParseError> {
        let start = self.position;
        self.advance(); // Skip the opening quote

        let mut content = String::new();
        loop {
            match self.current_char() {
                Some('"') => {
                    self.advance(); // Skip the closing quote
                    return Ok(Token::QuotedString(content));
                }
                Some(ch) => {
                    content.push(ch);
                    self.advance();
                }
                None => {
                    return Err(ParseError::UnexpectedToken {
                        position: start,
                        found: '"',
                    });
                }
            }
        }
    }

    /// Lexes an escaped character (\x).
    fn lex_escaped_char(&mut self) -> Result<Token, ParseError> {
        let start = self.position;
        self.advance(); // Skip the backslash

        match self.current_char() {
            Some(ch) => {
                self.advance();
                Ok(Token::EscapedChar(ch))
            }
            None => Err(ParseError::UnexpectedToken {
                position: start,
                found: '\\',
            }),
        }
    }

    /// Tries to match "General" keyword at the current position.
    /// Returns Some(SpannedToken) if a match is found, None otherwise.
    fn try_match_general(&mut self) -> Option<SpannedToken> {
        let remaining = self.remaining();
        let start = self.position;

        // Case-insensitive match without allocation
        // Use .get() for safe slicing that handles UTF-8 boundaries
        if let Some(prefix) = remaining.get(..7) {
            if prefix.eq_ignore_ascii_case("General") {
                // Match "General" keyword regardless of what follows
                // The parser will handle "General" followed by literals correctly
                self.position += 7; // len("General")
                return Some(SpannedToken {
                    token: Token::General,
                    start,
                    end: self.position,
                });
            }
        }
        None
    }

    /// Tries to match an AM/PM pattern at the current position.
    /// Returns Some(SpannedToken) if a match is found, None otherwise.
    fn try_match_am_pm(&mut self) -> Option<SpannedToken> {
        let remaining = self.remaining();
        let start = self.position;

        // Case-insensitive match without allocation (except for the final token string)
        // Use .get() for safe slicing that handles UTF-8 boundaries
        // Check longest patterns first
        if let Some(prefix) = remaining.get(..5) {
            if prefix.eq_ignore_ascii_case("AM/PM") {
                let matched = prefix.to_string();
                self.position += 5;
                return Some(SpannedToken {
                    token: Token::AmPm(matched),
                    start,
                    end: self.position,
                });
            }
        }
        // Malformed AM/P pattern (4 chars) - must check before A/P
        if let Some(prefix) = remaining.get(..4) {
            if prefix.eq_ignore_ascii_case("AM/P") {
                let matched = prefix.to_string();
                self.position += 4;
                return Some(SpannedToken {
                    token: Token::AmPm(matched),
                    start,
                    end: self.position,
                });
            }
        }
        if let Some(prefix) = remaining.get(..3) {
            if prefix.eq_ignore_ascii_case("A/P") {
                let matched = prefix.to_string();
                self.position += 3;
                return Some(SpannedToken {
                    token: Token::AmPm(matched),
                    start,
                    end: self.position,
                });
            }
        }
        None
    }

    /// Returns all remaining tokens as a vector.
    /// This consumes the lexer.
    pub fn tokenize(mut self) -> Result<Vec<SpannedToken>, ParseError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let is_eof = matches!(token.token, Token::Eof);
            tokens.push(token);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let mut lexer = Lexer::new("");
        assert!(matches!(lexer.next_token().unwrap().token, Token::Eof));
    }

    #[test]
    fn test_single_zero() {
        let mut lexer = Lexer::new("0");
        assert!(matches!(lexer.next_token().unwrap().token, Token::Zero));
        assert!(matches!(lexer.next_token().unwrap().token, Token::Eof));
    }
}
