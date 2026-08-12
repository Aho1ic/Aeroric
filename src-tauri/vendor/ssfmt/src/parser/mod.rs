//! Parser for ECMA-376 number format codes.

pub mod lexer;
pub mod tokens;

use crate::ast::{
    AmPmStyle, Color, Condition, DatePart, DigitPlaceholder, ElapsedPart, FormatPart, LocaleCode,
    NamedColor, NumberFormat, Section,
};
use crate::error::ParseError;
use lexer::Lexer;
use tokens::{SpannedToken, Token};

/// Parse a format code string into a NumberFormat.
pub fn parse(format_code: &str) -> Result<NumberFormat, ParseError> {
    if format_code.is_empty() {
        return Err(ParseError::EmptyFormat);
    }

    // Handle "General" format specially - it's Excel's default format
    // that displays numbers without unnecessary formatting
    // Also handle "[Color]General" and similar patterns
    let general_check = if format_code.eq_ignore_ascii_case("General") {
        Some(None) // General with no color
    } else if let Some(bracket_end) = format_code.find(']') {
        // Check if format is "[...]General"
        let after_bracket = &format_code[bracket_end + 1..];
        if after_bracket.trim().eq_ignore_ascii_case("General") {
            // Try to parse the bracket content as a color
            let bracket_content = &format_code[1..bracket_end];
            let color = try_parse_color(bracket_content);
            Some(color)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(color) = general_check {
        // Create an empty section that will trigger fallback formatting
        let general_section = Section {
            condition: None,
            color,
            parts: Vec::new(),
            metadata: crate::ast::SectionMetadata::default(),
        };
        return Ok(NumberFormat::from_sections(vec![general_section]));
    }

    let mut parser = Parser::new(format_code);
    parser.parse()
}

/// Parser for format code strings.
struct Parser<'a> {
    lexer: Lexer<'a>,
    /// Current token
    current: SpannedToken,
    /// Whether we've seen an hour token in the current section (for minute vs month disambiguation)
    seen_hour: bool,
}

impl<'a> Parser<'a> {
    /// Create a new parser for the given format code.
    fn new(format_code: &'a str) -> Self {
        let mut lexer = Lexer::new(format_code);
        // Get the first token
        let current = lexer.next_token().unwrap_or(SpannedToken {
            token: Token::Eof,
            start: 0,
            end: 0,
        });
        Self {
            lexer,
            current,
            seen_hour: false,
        }
    }

    /// Advance to the next token.
    fn advance(&mut self) -> Result<(), ParseError> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    /// Parse the format code into a NumberFormat.
    fn parse(&mut self) -> Result<NumberFormat, ParseError> {
        let mut sections = Vec::new();

        loop {
            let section = self.parse_section()?;
            sections.push(section);

            // Check for section separator or end
            if matches!(self.current.token, Token::Eof) {
                break;
            }

            if matches!(self.current.token, Token::SectionSep) {
                self.advance()?;
                // Continue to next section
            } else {
                break;
            }
        }

        Ok(NumberFormat::from_sections(sections))
    }

    /// Parse a single section of the format.
    fn parse_section(&mut self) -> Result<Section, ParseError> {
        let mut builder = SectionBuilder::new();
        self.seen_hour = false;

        loop {
            match &self.current.token {
                Token::Eof | Token::SectionSep => break,

                // General format keyword - return empty section to trigger fallback formatting
                // But only if General is the ONLY content (after color/condition)
                Token::General => {
                    self.advance()?;
                    // Check if there are more format parts after "General"
                    if matches!(self.current.token, Token::Eof | Token::SectionSep) {
                        // Truly just "General" - return empty section for fallback formatting
                        break;
                    } else {
                        // "General" followed by more content (like "General ")
                        // Add GeneralNumber part to signal General formatting should be used
                        builder.add_part(FormatPart::GeneralNumber);
                        // Continue parsing the rest as literals
                    }
                }

                // Bracket content - could be color, condition, elapsed time, or locale
                Token::OpenBracket => {
                    let bracket_start = self.current.start;
                    self.advance()?;
                    self.parse_bracket_content(&mut builder, bracket_start)?;
                }

                // Digit placeholders
                Token::Zero => {
                    builder.add_part(FormatPart::Digit(DigitPlaceholder::Zero));
                    self.advance()?;
                }
                Token::Hash => {
                    builder.add_part(FormatPart::Digit(DigitPlaceholder::Hash));
                    self.advance()?;
                }
                Token::Question => {
                    builder.add_part(FormatPart::Digit(DigitPlaceholder::Question));
                    self.advance()?;
                }

                // Separators
                Token::DecimalPoint => {
                    builder.add_part(FormatPart::DecimalPoint);
                    self.advance()?;
                }
                Token::ThousandsSep => {
                    builder.add_part(FormatPart::ThousandsSeparator);
                    self.advance()?;
                }

                // Special characters
                Token::Percent => {
                    builder.add_part(FormatPart::Percent);
                    self.advance()?;
                }
                Token::At => {
                    builder.add_part(FormatPart::TextPlaceholder);
                    self.advance()?;
                }
                Token::Asterisk => {
                    // Fill character - next char is the fill
                    self.advance()?;
                    if let Some(ch) = self.get_literal_char() {
                        builder.add_part(FormatPart::Fill(ch));
                        self.advance()?;
                    }
                }
                Token::Underscore => {
                    // Skip character - next char is the skip width
                    self.advance()?;
                    if let Some(ch) = self.get_literal_char() {
                        builder.add_part(FormatPart::Skip(ch));
                        self.advance()?;
                    }
                }

                // Scientific notation - but only if followed by + or -
                // Otherwise, check if it's an era year (e/E patterns like 'e', 'ee', 'eee')
                Token::ExponentUpper | Token::ExponentLower => {
                    let is_lower = matches!(self.current.token, Token::ExponentLower);
                    self.advance()?;

                    // Check if followed by + or - (scientific notation) or just a literal/date part
                    if matches!(self.current.token, Token::Plus | Token::Minus) {
                        let show_plus = matches!(self.current.token, Token::Plus);
                        self.advance()?;
                        let upper = !is_lower;
                        builder.add_part(FormatPart::Scientific { upper, show_plus });
                    } else {
                        // Standalone 'e' or 'E' - could be era year (date format)
                        // Skip consecutive e/E tokens
                        while matches!(
                            self.current.token,
                            Token::ExponentLower | Token::ExponentUpper
                        ) {
                            self.advance()?;
                        }

                        // Era year format: e, ee, eee, eeee all output 4-digit year
                        // For Gregorian calendar, era year is same as regular year
                        // Excel always shows the full year for 'e' format
                        builder.add_part(FormatPart::DatePart(DatePart::Year4));
                    }
                }

                // Signs become literals in format context (when not part of scientific notation)
                Token::Plus => {
                    builder.add_part(FormatPart::Literal("+".to_string()));
                    self.advance()?;
                }
                Token::Minus => {
                    builder.add_part(FormatPart::Literal("-".to_string()));
                    self.advance()?;
                }

                // Fraction
                Token::Slash => {
                    builder.add_part(FormatPart::Literal("/".to_string()));
                    self.advance()?;
                }

                // Date/time tokens
                Token::Year => {
                    let count = self.count_consecutive(&Token::Year)?;
                    let part = if count >= 4 {
                        DatePart::Year4
                    } else if count == 3 {
                        DatePart::Year3
                    } else {
                        DatePart::Year2
                    };
                    builder.add_part(FormatPart::DatePart(part));
                }
                Token::Month => {
                    // Check if this should be minute (after hour) or month
                    // BEFORE consuming tokens, check if seconds follow
                    let has_seconds_following = self.has_seconds_ahead();
                    let count = self.count_consecutive(&Token::Month)?;
                    // It's a minute if:
                    // 1. We've seen an hour token, OR
                    // 2. There are seconds tokens following (mm:ss pattern)
                    let part = if self.seen_hour || has_seconds_following {
                        // This is minute
                        if count >= 2 {
                            DatePart::Minute2
                        } else {
                            DatePart::Minute
                        }
                    } else {
                        // This is month
                        match count {
                            1 => DatePart::Month,
                            2 => DatePart::Month2,
                            3 => DatePart::MonthAbbr,
                            4 => DatePart::MonthFull,
                            _ => DatePart::MonthLetter,
                        }
                    };
                    builder.add_part(FormatPart::DatePart(part));
                }
                Token::Day => {
                    let count = self.count_consecutive(&Token::Day)?;
                    let part = match count {
                        1 => DatePart::Day,
                        2 => DatePart::Day2,
                        3 => DatePart::DayAbbr,
                        _ => DatePart::DayFull,
                    };
                    builder.add_part(FormatPart::DatePart(part));
                }
                Token::Hour => {
                    self.seen_hour = true;
                    let count = self.count_consecutive(&Token::Hour)?;
                    let part = if count >= 2 {
                        DatePart::Hour2
                    } else {
                        DatePart::Hour
                    };
                    builder.add_part(FormatPart::DatePart(part));

                    // Check for fractional hours (.0, .00, .000, etc.)
                    if matches!(self.current.token, Token::DecimalPoint) {
                        self.advance()?;
                        // Count consecutive zeros after decimal point
                        let mut frac_places = 0;
                        while matches!(self.current.token, Token::Zero) {
                            frac_places += 1;
                            self.advance()?;
                        }
                        if frac_places > 0 {
                            // Add decimal point as literal
                            builder.add_part(FormatPart::Literal(".".to_string()));
                            // Treat as subsecond for now (fractional time)
                            builder.add_part(FormatPart::DatePart(DatePart::SubSecond(
                                frac_places as u8,
                            )));
                        }
                    }
                }
                Token::Second => {
                    let count = self.count_consecutive(&Token::Second)?;
                    let part = if count >= 2 {
                        DatePart::Second2
                    } else {
                        DatePart::Second
                    };
                    builder.add_part(FormatPart::DatePart(part));

                    // Check for subsecond formatting (.0, .00, .000, etc.)
                    if matches!(self.current.token, Token::DecimalPoint) {
                        self.advance()?;
                        // Count consecutive zeros after decimal point
                        let mut subsec_places = 0;
                        while matches!(self.current.token, Token::Zero) {
                            subsec_places += 1;
                            self.advance()?;
                        }
                        if subsec_places > 0 {
                            // Add decimal point as literal
                            builder.add_part(FormatPart::Literal(".".to_string()));
                            builder.add_part(FormatPart::DatePart(DatePart::SubSecond(
                                subsec_places as u8,
                            )));
                        }
                    }
                }

                // Buddhist calendar
                Token::BuddhistYear => {
                    let count = self.count_consecutive(&Token::BuddhistYear)?;
                    let part = if count >= 4 {
                        DatePart::BuddhistYear4
                    } else {
                        DatePart::BuddhistYear2
                    };
                    builder.add_part(FormatPart::DatePart(part));
                }
                Token::BuddhistYearUpper => {
                    self.advance()?;
                    // Check if this is 'B2' format (alternative Buddhist calendar)
                    if matches!(self.current.token, Token::Literal('2')) {
                        self.advance()?;
                        // B2 is a prefix that modifies subsequent year formatting
                        // Check if followed by year tokens and convert them to BuddhistYear*Alt
                        if matches!(self.current.token, Token::Year) {
                            let count = self.count_consecutive(&Token::Year)?;
                            if count >= 4 {
                                // B2yyyy -> use alternative Buddhist calendar for 4-digit year
                                builder.add_part(FormatPart::DatePart(DatePart::BuddhistYear4Alt));
                            } else {
                                // B2yy -> use 2-digit alternative Buddhist year
                                builder.add_part(FormatPart::DatePart(DatePart::BuddhistYear2Alt));
                            }
                        } else {
                            // B2 not followed by year - treat as literal
                            builder.add_part(FormatPart::Literal("B2".to_string()));
                        }
                    } else {
                        // Just 'B' by itself - treat as regular Buddhist year
                        let count = 1 + self.count_consecutive(&Token::BuddhistYearUpper)?;
                        let part = if count >= 4 {
                            DatePart::BuddhistYear4
                        } else {
                            DatePart::BuddhistYear2
                        };
                        builder.add_part(FormatPart::DatePart(part));
                    }
                }

                // AM/PM
                Token::AmPm(s) => {
                    let style = parse_am_pm_style(s);
                    builder.add_part(FormatPart::AmPm(style));
                    self.advance()?;
                }

                // Literals
                Token::Literal(ch) => {
                    builder.add_part(FormatPart::Literal(ch.to_string()));
                    self.advance()?;
                }
                Token::EscapedChar(ch) => {
                    builder.add_part(FormatPart::EscapedLiteral(ch.to_string()));
                    self.advance()?;
                }
                Token::QuotedString(s) => {
                    builder.add_part(FormatPart::Literal(s.clone()));
                    self.advance()?;
                }

                Token::CloseBracket => {
                    // Unexpected close bracket - treat as literal
                    builder.add_part(FormatPart::Literal("]".to_string()));
                    self.advance()?;
                }
            }
        }

        Ok(builder.build())
    }

    /// Parse bracket content: [Red], [>100], [h], [$-409], etc.
    fn parse_bracket_content(
        &mut self,
        builder: &mut SectionBuilder,
        bracket_start: usize,
    ) -> Result<(), ParseError> {
        // Collect all content until we hit the close bracket
        let mut content = String::new();

        loop {
            match &self.current.token {
                Token::CloseBracket => {
                    self.advance()?;
                    break;
                }
                Token::Eof => {
                    return Err(ParseError::UnterminatedBracket {
                        position: bracket_start,
                    });
                }
                Token::Literal(ch) => {
                    content.push(*ch);
                    self.advance()?;
                }
                // Other tokens that might appear inside brackets
                Token::Zero => {
                    content.push('0');
                    self.advance()?;
                }
                Token::Hash => {
                    content.push('#');
                    self.advance()?;
                }
                Token::Question => {
                    content.push('?');
                    self.advance()?;
                }
                Token::DecimalPoint => {
                    content.push('.');
                    self.advance()?;
                }
                Token::ThousandsSep => {
                    content.push(',');
                    self.advance()?;
                }
                Token::Percent => {
                    content.push('%');
                    self.advance()?;
                }
                Token::At => {
                    content.push('@');
                    self.advance()?;
                }
                Token::Asterisk => {
                    content.push('*');
                    self.advance()?;
                }
                Token::Underscore => {
                    content.push('_');
                    self.advance()?;
                }
                Token::Plus => {
                    content.push('+');
                    self.advance()?;
                }
                Token::Minus => {
                    content.push('-');
                    self.advance()?;
                }
                Token::Slash => {
                    content.push('/');
                    self.advance()?;
                }
                Token::ExponentUpper => {
                    content.push('E');
                    self.advance()?;
                }
                Token::ExponentLower => {
                    content.push('e');
                    self.advance()?;
                }
                _ => {
                    // Skip other tokens inside brackets
                    self.advance()?;
                }
            }
        }

        // Now parse the bracket content
        let content = content.trim();

        // Try to parse as color
        if let Some(color) = try_parse_color(content) {
            builder.color = Some(color);
            return Ok(());
        }

        // Try to parse as condition
        if let Some(condition) = try_parse_condition(content) {
            builder.condition = Some(condition);
            return Ok(());
        }

        // Try to parse as elapsed time
        if let Some(elapsed) = try_parse_elapsed(content) {
            builder.add_part(FormatPart::Elapsed(elapsed));
            // If this is elapsed hours, set seen_hour so that subsequent 'mm' is parsed as minutes
            if matches!(elapsed, ElapsedPart::Hours | ElapsedPart::Hours2) {
                self.seen_hour = true;
            }
            return Ok(());
        }

        // Try to parse as locale code
        if let Some(locale) = try_parse_locale(content) {
            builder.add_part(FormatPart::Locale(locale));
            return Ok(());
        }

        // Unknown bracket content - treat as literal (or ignore)
        Ok(())
    }

    /// Count consecutive tokens of the same type and advance past them.
    fn count_consecutive(&mut self, token_type: &Token) -> Result<usize, ParseError> {
        let mut count = 0;
        while self.token_matches(token_type) {
            count += 1;
            self.advance()?;
        }
        Ok(count)
    }

    /// Check if current token matches the given token type (ignoring content).
    fn token_matches(&self, token_type: &Token) -> bool {
        std::mem::discriminant(&self.current.token) == std::mem::discriminant(token_type)
    }

    /// Check if there are seconds tokens appearing immediately after in a time context.
    /// Used to disambiguate mm:ss (minutes:seconds) from mm-dd (month-day).
    /// Returns true if 's' or 'S' appears next (with or without colon), indicating time format.
    /// Note: Called while current token is a Month token, so need to skip past 'm'/'M' chars.
    fn has_seconds_ahead(&self) -> bool {
        // Look ahead in the lexer's remaining input, starting from current position
        let mut remaining = &self.lexer.input[self.current.start..];

        // Skip past 'm' or 'M' characters (the month/minute tokens we're currently at)
        remaining = remaining.trim_start_matches(['m', 'M']);
        remaining = remaining.trim_start();

        // Check for time context patterns:
        // 1. ":s" or ":ss" - minutes:seconds with colon (e.g., "mm:ss")
        // 2. "s" or "ss" - minutes+seconds without colon (e.g., "mmss.0")
        if let Some(after_colon) = remaining.strip_prefix(':') {
            // Check if 's' or 'S' follows the colon
            if let Some(first_ch) = after_colon.chars().next() {
                return first_ch == 's' || first_ch == 'S';
            }
        } else if let Some(first_ch) = remaining.chars().next() {
            // Check if 's' or 'S' follows immediately (without colon)
            return first_ch == 's' || first_ch == 'S';
        }

        false
    }

    /// Get the literal character from the current token.
    fn get_literal_char(&self) -> Option<char> {
        match &self.current.token {
            Token::Literal(ch) => Some(*ch),
            Token::Zero => Some('0'),
            Token::Hash => Some('#'),
            Token::Question => Some('?'),
            Token::DecimalPoint => Some('.'),
            Token::ThousandsSep => Some(','),
            Token::Percent => Some('%'),
            Token::At => Some('@'),
            Token::Asterisk => Some('*'),
            Token::Underscore => Some('_'),
            Token::Plus => Some('+'),
            Token::Minus => Some('-'),
            Token::Slash => Some('/'),
            Token::EscapedChar(ch) => Some(*ch),
            _ => None,
        }
    }
}

/// Helper struct for building sections.
struct SectionBuilder {
    condition: Option<Condition>,
    color: Option<Color>,
    parts: Vec<FormatPart>,
}

impl SectionBuilder {
    fn new() -> Self {
        Self {
            condition: None,
            color: None,
            parts: Vec::new(),
        }
    }

    fn add_part(&mut self, part: FormatPart) {
        self.parts.push(part);
    }

    fn build(mut self) -> Section {
        // Post-process to detect fraction patterns
        self.detect_fractions();

        // Post-process to detect subsecond patterns in date formats
        self.detect_subseconds();

        // Compute metadata by scanning the parts once
        let metadata = self.compute_metadata();

        Section {
            condition: self.condition,
            color: self.color,
            parts: self.parts,
            metadata,
        }
    }

    /// Compute section metadata by scanning parts once
    /// Based on SSF's eval_fmt in bits/82_eval.js
    fn compute_metadata(&self) -> crate::ast::SectionMetadata {
        use crate::ast::*;

        let mut has_ampm = false;
        let mut is_hijri = false;
        let mut max_subsecond_precision = None;
        let mut has_elapsed_time = false;
        let mut smallest_time_unit = TimeUnit::None;
        let mut format_type = FormatType::General;

        // Scan parts to gather metadata
        for part in &self.parts {
            match part {
                FormatPart::AmPm(_) => {
                    has_ampm = true;
                }
                FormatPart::DatePart(DatePart::BuddhistYear4Alt | DatePart::BuddhistYear2Alt) => {
                    is_hijri = true;
                }
                FormatPart::DatePart(DatePart::SubSecond(precision)) => {
                    max_subsecond_precision = Some(max_subsecond_precision.unwrap_or(0).max(*precision));
                    if smallest_time_unit < TimeUnit::Subseconds {
                        smallest_time_unit = TimeUnit::Subseconds;
                    }
                }
                FormatPart::DatePart(DatePart::Second | DatePart::Second2) => {
                    if smallest_time_unit < TimeUnit::Seconds {
                        smallest_time_unit = TimeUnit::Seconds;
                    }
                }
                FormatPart::DatePart(DatePart::Minute | DatePart::Minute2) => {
                    if smallest_time_unit < TimeUnit::Minutes {
                        smallest_time_unit = TimeUnit::Minutes;
                    }
                }
                FormatPart::DatePart(DatePart::Hour | DatePart::Hour2) => {
                    if smallest_time_unit < TimeUnit::Hours {
                        smallest_time_unit = TimeUnit::Hours;
                    }
                }
                FormatPart::Elapsed(_) => {
                    has_elapsed_time = true;
                }
                FormatPart::Fraction { .. } => {
                    format_type = FormatType::Fraction;
                }
                FormatPart::TextPlaceholder => {
                    format_type = FormatType::Text;
                }
                _ => {}
            }
        }

        // Determine format type if not already set
        if format_type == FormatType::General {
            let has_date = self.parts.iter().any(|p| matches!(p, FormatPart::DatePart(_)));
            let has_number = self.parts.iter().any(|p| matches!(p, FormatPart::Digit(_) | FormatPart::DecimalPoint));

            if has_date || has_ampm || has_elapsed_time {
                format_type = FormatType::DateTime;
            } else if has_number {
                format_type = FormatType::Number;
            }
        }

        SectionMetadata {
            has_ampm,
            is_hijri,
            max_subsecond_precision,
            has_elapsed_time,
            smallest_time_unit,
            format_type,
        }
    }

    /// Detect and merge fraction patterns in the parts list.
    /// Looks for patterns like: [digits] "/" [digits] and converts to Fraction
    fn detect_fractions(&mut self) {
        let mut new_parts = Vec::new();
        let mut i = 0;

        while i < self.parts.len() {
            // Look for a "/" literal that could be part of a fraction
            if let Some(slash_pos) = self.find_slash_position(i) {
                // Check if there are digit placeholders or a fixed number after the slash
                // Skip any spaces after the slash
                let mut denom_start = slash_pos + 1;
                while denom_start < self.parts.len() {
                    if let FormatPart::Literal(s) = &self.parts[denom_start] {
                        if s.chars().all(|c| c == ' ') {
                            denom_start += 1;
                            continue;
                        }
                    }
                    break;
                }

                let denom_digits = self.collect_digit_placeholders(denom_start);

                // Also check for fixed denominator (numeric literal)
                // Need to collect consecutive digit literals/digits to handle multi-digit numbers like "10", "16", etc.
                let (fixed_denom, fixed_denom_len) = if denom_digits.is_empty() {
                    let mut num_str = String::new();
                    let mut count = 0;
                    for i in denom_start..self.parts.len() {
                        match &self.parts[i] {
                            FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) if s.len() == 1 && s.chars().next().unwrap().is_ascii_digit() => {
                                // Single digit literal like "1", "6"
                                num_str.push_str(s);
                                count += 1;
                            }
                            FormatPart::Digit(DigitPlaceholder::Zero) => {
                                // "0" token - can be part of a fixed denominator number
                                num_str.push('0');
                                count += 1;
                            }
                            _ => {
                                break;
                            }
                        }
                    }
                    if !num_str.is_empty() {
                        (num_str.parse::<u32>().ok(), count)
                    } else {
                        (None, 0)
                    }
                } else {
                    (None, 0)
                };

                if !denom_digits.is_empty() || fixed_denom.is_some() {
                    // Found denominator, now look for numerator before slash
                    // Skip any spaces before the slash
                    let mut num_search_pos = slash_pos;
                    while num_search_pos > 0 {
                        if let FormatPart::Literal(s) = &self.parts[num_search_pos - 1] {
                            if s.chars().all(|c| c == ' ') {
                                num_search_pos -= 1;
                                continue;
                            }
                        }
                        break;
                    }

                    let num_end = num_search_pos;
                    if num_end > 0 {
                        let num_digits = self.collect_digit_placeholders_reverse(num_end - 1);

                        if !num_digits.is_empty() {
                            // Found numerator, now collect any integer part before that
                            let num_start = num_end - num_digits.len();
                            let mut int_digits = if num_start > 0 {
                                self.collect_integer_part(num_start - 1, &mut new_parts)
                            } else {
                                Vec::new()
                            };

                            // Check if this is a mixed fraction or improper fraction
                            // Mixed fraction: has space between integer and numerator (e.g., "# ??/??")
                            // Improper fraction: no space, all digits before slash are numerator (e.g., "#0#00??/??")
                            let has_space_before_numerator = num_start > 0 && {
                                if let Some(FormatPart::Literal(s) | FormatPart::EscapedLiteral(s)) = self.parts.get(num_start - 1) {
                                    s == " "
                                } else {
                                    false
                                }
                            };

                            // For improper fractions, combine int_digits and num_digits into numerator
                            let (final_int_digits, final_num_digits) = if !has_space_before_numerator && !int_digits.is_empty() {
                                // Improper fraction: ALL digits before slash are numerator digits
                                int_digits.extend(num_digits);
                                (Vec::new(), int_digits)
                            } else {
                                // Mixed fraction: keep them separate
                                (int_digits, num_digits)
                            };

                            // Check for spaces before slash (between numerator and slash)
                            let space_before_slash = if num_start > 0 && slash_pos > num_start {
                                if let FormatPart::Literal(s) = &self.parts[slash_pos - 1] {
                                    if s.chars().all(|c| c == ' ') {
                                        s.clone()
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };

                            // Check for spaces after slash (between slash and denominator)
                            let space_after_slash = if slash_pos + 1 < denom_start {
                                if let FormatPart::Literal(s) = &self.parts[slash_pos + 1] {
                                    if s.chars().all(|c| c == ' ') {
                                        s.clone()
                                    } else {
                                        String::new()
                                    }
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            };

                            // Create fraction part
                            let denominator = if let Some(fixed) = fixed_denom {
                                crate::ast::FractionDenom::Fixed(fixed)
                            } else {
                                crate::ast::FractionDenom::UpToDigits(denom_digits.len() as u8)
                            };

                            let fraction = FormatPart::Fraction {
                                integer_digits: final_int_digits,
                                numerator_digits: final_num_digits,
                                denominator,
                                space_before_slash,
                                space_after_slash,
                            };
                            new_parts.push(fraction);

                            // Skip past all the parts we consumed
                            let skip_count = if fixed_denom.is_some() {
                                fixed_denom_len // Skip all the fixed denominator literals
                            } else {
                                denom_digits.len() // Skip all denominator digit placeholders
                            };
                            i = denom_start + skip_count;
                            continue;
                        }
                    }
                }
            }

            // Not part of a fraction, keep the part as-is
            if i < self.parts.len() {
                new_parts.push(self.parts[i].clone());
                i += 1;
            }
        }

        self.parts = new_parts;
    }

    /// Detect and convert subsecond patterns in date formats.
    /// Looks for DecimalPoint followed by Digit(Zero) placeholders after date/time parts
    /// and converts them to Literal(".") + DatePart::SubSecond(n).
    fn detect_subseconds(&mut self) {
        let mut new_parts = Vec::new();
        let mut i = 0;

        while i < self.parts.len() {
            // Check if current part is a DecimalPoint
            if matches!(&self.parts[i], FormatPart::DecimalPoint) {
                // Check if there are consecutive Zero digit placeholders after it
                let mut zero_count = 0;
                let mut j = i + 1;
                while j < self.parts.len() && matches!(&self.parts[j], FormatPart::Digit(DigitPlaceholder::Zero)) {
                    zero_count += 1;
                    j += 1;
                }

                // If we found zeros after the decimal point, check if there are date/time parts before
                if zero_count > 0 {
                    let has_date_parts = new_parts.iter().any(|p| matches!(p,
                        FormatPart::DatePart(_) | FormatPart::AmPm(_) | FormatPart::Elapsed(_)
                    ));

                    if has_date_parts {
                        // Convert to subsecond formatting
                        new_parts.push(FormatPart::Literal(".".to_string()));
                        new_parts.push(FormatPart::DatePart(DatePart::SubSecond(zero_count as u8)));
                        i = j; // Skip past the decimal point and zeros
                        continue;
                    }
                }
            }

            // Not a subsecond pattern, keep the part as-is
            new_parts.push(self.parts[i].clone());
            i += 1;
        }

        self.parts = new_parts;
    }

    /// Find position of "/" literal starting from index
    fn find_slash_position(&self, start: usize) -> Option<usize> {
        for i in start..self.parts.len() {
            if matches!(&self.parts[i], FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) if s == "/") {
                return Some(i);
            }
        }
        None
    }

    /// Collect consecutive digit placeholders starting from index
    fn collect_digit_placeholders(&self, start: usize) -> Vec<DigitPlaceholder> {
        let mut digits = Vec::new();
        for i in start..self.parts.len() {
            if let FormatPart::Digit(d) = &self.parts[i] {
                digits.push(*d);
            } else {
                break;
            }
        }
        digits
    }

    /// Collect consecutive digit placeholders in reverse from index
    fn collect_digit_placeholders_reverse(&self, end: usize) -> Vec<DigitPlaceholder> {
        let mut digits = Vec::new();
        let mut i = end as isize;
        while i >= 0 {
            if let Some(FormatPart::Digit(d)) = self.parts.get(i as usize) {
                digits.push(*d);
            } else {
                break;
            }
            i -= 1;
        }
        digits.reverse();
        digits
    }

    /// Collect integer part before numerator (digits before a space typically)
    fn collect_integer_part(&self, end: usize, new_parts: &mut Vec<FormatPart>) -> Vec<DigitPlaceholder> {
        let mut int_digits = Vec::new();

        // Scan backwards from end to find digit placeholders
        let mut i = end as isize;
        let mut found_space = false;

        while i >= 0 {
            match &self.parts.get(i as usize) {
                Some(FormatPart::Digit(d)) => {
                    int_digits.push(*d);
                }
                Some(FormatPart::Literal(s) | FormatPart::EscapedLiteral(s)) if s == " " => {
                    // Found a space - this indicates a mixed fraction
                    found_space = true;
                    if !int_digits.is_empty() {
                        // Already collected some integer digits, we're done
                        break;
                    }
                    // Haven't collected integer digits yet, continue scanning backwards
                }
                Some(FormatPart::ThousandsSeparator) if !int_digits.is_empty() => {
                    // Allow thousands separator in integer part
                }
                Some(FormatPart::Literal(_) | FormatPart::EscapedLiteral(_)) if int_digits.is_empty() => {
                    // Haven't started collecting digits yet, and it's not a space, keep this part
                }
                _ => {
                    if !int_digits.is_empty() {
                        break;
                    }
                }
            }
            i -= 1;
        }

        // If we found integer digits, remove them from new_parts
        if !int_digits.is_empty() && found_space {
            int_digits.reverse();
            // Remove parts from where integer starts
            let remove_from = (i + 1) as usize;
            new_parts.truncate(remove_from);
        } else {
            int_digits.clear();
        }

        int_digits
    }
}

/// Parse AM/PM style from the matched string.
fn parse_am_pm_style(s: &str) -> AmPmStyle {
    match s {
        "AM/PM" => AmPmStyle::Upper,
        "am/pm" => AmPmStyle::Lower,
        "AM/P" => AmPmStyle::MalformedUpper,
        "am/p" => AmPmStyle::MalformedLower,
        "A/P" => AmPmStyle::ShortUpper,
        "a/p" => AmPmStyle::ShortLower,
        // Default to upper for mixed case
        _ => {
            if s.len() == 4 {
                // 4-char patterns like "Am/P" - treat as malformed
                if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    AmPmStyle::MalformedUpper
                } else {
                    AmPmStyle::MalformedLower
                }
            } else if s.len() <= 3 {
                if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                    AmPmStyle::ShortUpper
                } else {
                    AmPmStyle::ShortLower
                }
            } else if s.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                AmPmStyle::Upper
            } else {
                AmPmStyle::Lower
            }
        }
    }
}

/// Try to parse bracket content as a color.
fn try_parse_color(content: &str) -> Option<Color> {
    // Check for named colors
    if let Ok(named) = content.parse::<NamedColor>() {
        return Some(Color::Named(named));
    }

    // Check for indexed colors: Color1 through Color56
    let lower = content.to_lowercase();
    if lower.starts_with("color") {
        if let Ok(index) = content[5..].parse::<u8>() {
            if (1..=56).contains(&index) {
                return Some(Color::Indexed(index));
            }
        }
    }

    None
}

/// Try to parse bracket content as a condition.
fn try_parse_condition(content: &str) -> Option<Condition> {
    let content = content.trim();

    // Parse conditions like >=, <=, <>, >, <, =
    if let Some(value_str) = content.strip_prefix(">=") {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::GreaterOrEqual(value));
        }
    } else if let Some(value_str) = content.strip_prefix("<=") {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::LessOrEqual(value));
        }
    } else if let Some(value_str) = content.strip_prefix("<>") {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::NotEqual(value));
        }
    } else if let Some(value_str) = content.strip_prefix('>') {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::GreaterThan(value));
        }
    } else if let Some(value_str) = content.strip_prefix('<') {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::LessThan(value));
        }
    } else if let Some(value_str) = content.strip_prefix('=') {
        if let Ok(value) = value_str.trim().parse::<f64>() {
            return Some(Condition::Equal(value));
        }
    }

    None
}

/// Try to parse bracket content as elapsed time.
fn try_parse_elapsed(content: &str) -> Option<ElapsedPart> {
    let lower = content.to_lowercase();
    match lower.as_str() {
        "h" => Some(ElapsedPart::Hours),
        "hh" => Some(ElapsedPart::Hours2),
        "m" => Some(ElapsedPart::Minutes),
        "mm" => Some(ElapsedPart::Minutes2),
        "s" => Some(ElapsedPart::Seconds),
        "ss" => Some(ElapsedPart::Seconds2),
        _ => None,
    }
}

/// Try to parse bracket content as a locale code.
fn try_parse_locale(content: &str) -> Option<LocaleCode> {
    // Locale codes start with $ e.g., [$-409], [$€-407]
    if !content.starts_with('$') {
        return None;
    }

    let rest = &content[1..];

    // Parse [$currency-lcid] format
    if let Some(dash_pos) = rest.find('-') {
        let currency_part = &rest[..dash_pos];
        let lcid_part = &rest[dash_pos + 1..];

        let currency = if currency_part.is_empty() {
            None
        } else {
            Some(currency_part.to_string())
        };

        let lcid = u32::from_str_radix(lcid_part, 16).ok();

        Some(LocaleCode { currency, lcid })
    } else {
        // Just a currency symbol
        Some(LocaleCode {
            currency: if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            },
            lcid: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let result = parse("");
        assert!(matches!(result, Err(ParseError::EmptyFormat)));
    }

    #[test]
    fn test_parse_single_zero() {
        let fmt = parse("0").unwrap();
        assert_eq!(fmt.sections().len(), 1);
        assert_eq!(fmt.sections()[0].parts.len(), 1);
    }

    #[test]
    fn test_try_parse_color_named() {
        assert!(matches!(
            try_parse_color("Red"),
            Some(Color::Named(NamedColor::Red))
        ));
        assert!(matches!(
            try_parse_color("blue"),
            Some(Color::Named(NamedColor::Blue))
        ));
    }

    #[test]
    fn test_try_parse_color_indexed() {
        assert!(matches!(try_parse_color("Color1"), Some(Color::Indexed(1))));
        assert!(matches!(
            try_parse_color("Color56"),
            Some(Color::Indexed(56))
        ));
        assert!(try_parse_color("Color0").is_none());
        assert!(try_parse_color("Color57").is_none());
    }

    #[test]
    fn test_try_parse_condition() {
        assert!(matches!(
            try_parse_condition(">100"),
            Some(Condition::GreaterThan(n)) if (n - 100.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            try_parse_condition("<0"),
            Some(Condition::LessThan(n)) if n.abs() < f64::EPSILON
        ));
        assert!(matches!(
            try_parse_condition(">=50"),
            Some(Condition::GreaterOrEqual(n)) if (n - 50.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            try_parse_condition("<=10"),
            Some(Condition::LessOrEqual(n)) if (n - 10.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            try_parse_condition("=5"),
            Some(Condition::Equal(n)) if (n - 5.0).abs() < f64::EPSILON
        ));
        assert!(matches!(
            try_parse_condition("<>0"),
            Some(Condition::NotEqual(n)) if n.abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_try_parse_elapsed() {
        assert!(matches!(try_parse_elapsed("h"), Some(ElapsedPart::Hours)));
        assert!(matches!(try_parse_elapsed("hh"), Some(ElapsedPart::Hours2)));
        assert!(matches!(try_parse_elapsed("m"), Some(ElapsedPart::Minutes)));
        assert!(matches!(
            try_parse_elapsed("mm"),
            Some(ElapsedPart::Minutes2)
        ));
        assert!(matches!(try_parse_elapsed("s"), Some(ElapsedPart::Seconds)));
        assert!(matches!(
            try_parse_elapsed("ss"),
            Some(ElapsedPart::Seconds2)
        ));
    }

    #[test]
    fn test_try_parse_locale() {
        let locale = try_parse_locale("$-409").unwrap();
        assert!(locale.currency.is_none());
        assert_eq!(locale.lcid, Some(0x409));

        let locale = try_parse_locale("$€-407").unwrap();
        assert_eq!(locale.currency, Some("€".to_string()));
        assert_eq!(locale.lcid, Some(0x407));

        let locale = try_parse_locale("$$").unwrap();
        assert_eq!(locale.currency, Some("$".to_string()));
        assert!(locale.lcid.is_none());
    }
}
