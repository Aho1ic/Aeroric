//! Date and time formatting

use crate::ast::{AmPmStyle, DatePart, ElapsedPart, FormatPart, Section};
use crate::date_serial::{serial_to_date, serial_to_weekday};
use crate::error::FormatError;
use crate::locale::Locale;
use crate::options::FormatOptions;

/// Format a value as a date/time using the given section.
pub fn format_date(
    value: f64,
    section: &Section,
    opts: &FormatOptions,
) -> Result<String, FormatError> {
    // SSF returns empty string for out-of-range dates (< 0 or > 2958465)
    // This matches Excel's behavior - see bits/35_datecode.js line 2
    if !(0.0..=2958465.0).contains(&value) {
        return Ok(String::new());
    }

    // Use pre-computed metadata instead of scanning parts
    // Metadata is computed once during parsing for better performance
    let is_hijri = section.metadata.is_hijri;
    let has_ampm = section.metadata.has_ampm;

    // Check if there are multiple SubSecond parts (still need to scan for this specific case)
    let has_multiple_subseconds = section
        .parts
        .iter()
        .filter(|p| matches!(p, FormatPart::DatePart(DatePart::SubSecond(_))))
        .count()
        > 1;

    // Round the serial value if it's very close to an integer
    // This handles floating point precision errors like 2.9999999999999996 -> 3.0
    // which should display as 72:00:00 not 72:01:00
    let adjusted_value = if (value - value.round()).abs() < 1e-10 {
        value.round()
    } else {
        value
    };

    // Get date components
    // For time-only values (serial < 1), use a default date since we only need time
    let (mut year, mut month, mut day) = if value >= 1.0 {
        serial_to_date(value, opts.date_system)
            .ok_or(FormatError::DateOutOfRange { serial: value })?
    } else {
        // For time-only formatting, use day 0 to indicate no date component
        // Excel shows "1/0/00" for m/d/yy format with time-only values
        (1900, 1, 0)
    };

    // Apply Hijri calendar conversion if B2 prefix is used
    // Use the Kuwaiti algorithm for proper date conversion
    if is_hijri {
        let days = value.floor() as i64;
        if days == 60 {
            // Special case for Excel's fake leap day (Feb 29, 1900)
            // This date doesn't exist in the Gregorian calendar
            // SSF hardcodes this to 1317-10-29
            year = 1317;
            month = 10;
            day = 29;
        } else if days == 0 {
            // Special case for day 0 (Dec 31, 1899 in Excel's calendar)
            // SSF hardcodes this to 1317-08-29
            year = 1317;
            month = 8;
            day = 29;
        } else {
            // For all other dates, use proper Hijri calendar conversion
            let (hijri_year, hijri_month, hijri_day) =
                crate::hijri::gregorian_to_hijri(year, month, day);
            year = hijri_year;
            month = hijri_month;
            day = hijri_day;
        }
    }

    // Get time components
    // Only round seconds when there's no subsecond display in the format
    let has_subseconds = section.metadata.max_subsecond_precision.is_some();
    let (mut hour, mut minute, mut second) = crate::date_serial::serial_to_time_with_rounding(adjusted_value, !has_subseconds);

    // Apply pre-rounding based on smallest displayed time unit
    // This ensures proper rounding behavior (e.g., 12:34:59.9 displayed as "hh:mm" shows "12:35")
    // Only apply when we have subsecond display - otherwise, serial_to_time already rounded.
    if has_subseconds {
        let fraction = adjusted_value.fract().abs();
        // Round to millisecond precision first (same as serial_to_time_impl) to handle
        // floating point errors, then extract subseconds
        let total_seconds = (fraction * 86400.0 * 1000.0).round() / 1000.0;
        let subseconds = total_seconds - total_seconds.floor();

        apply_time_prerounding(
            &mut hour,
            &mut minute,
            &mut second,
            subseconds,
            section.metadata.smallest_time_unit,
            section.metadata.max_subsecond_precision,
        );
    }

    // Get weekday (1=Sunday...7=Saturday)
    // Always calculate weekday based on serial value
    // Even for value 0, Excel calculates it as Saturday (day before Jan 1, 1900)
    let weekday = serial_to_weekday(value, opts.date_system);

    // Build the formatted string
    let mut result = String::new();

    for part in &section.parts {
        match part {
            FormatPart::DatePart(date_part) => {
                let formatted = format_date_part(
                    *date_part,
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                    weekday,
                    has_ampm,
                    value, // Pass the original serial value for fractional seconds
                    has_multiple_subseconds,
                    &opts.locale,
                );
                result.push_str(&formatted);
            }
            FormatPart::AmPm(style) => {
                let formatted = format_ampm(*style, hour, &opts.locale);
                result.push_str(&formatted);
            }
            FormatPart::Elapsed(elapsed_part) => {
                let formatted = format_elapsed(*elapsed_part, adjusted_value);
                result.push_str(&formatted);
            }
            FormatPart::Literal(s) | FormatPart::EscapedLiteral(s) => {
                result.push_str(s);
            }
            FormatPart::Skip(c) => {
                // Skip width of character - add a space for alignment
                result.push(*c);
            }
            FormatPart::Fill(_) => {
                // Fill characters are handled at a higher level
                // For now, just skip
            }
            FormatPart::ThousandsSeparator => {
                // In date formats, the thousands separator (,) is just a literal comma
                result.push(opts.locale.thousands_separator);
            }
            FormatPart::DecimalPoint => {
                // In date formats, the decimal point is just a literal
                result.push(opts.locale.decimal_separator);
            }
            _ => {
                // Other parts (e.g., numeric) are not expected in date formats
                // but we'll ignore them silently
            }
        }
    }

    Ok(result)
}

/// Format a single date/time part.
#[allow(clippy::too_many_arguments)]
fn format_date_part(
    part: DatePart,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    weekday: u32,
    has_ampm: bool,
    serial: f64,
    has_multiple_subseconds: bool,
    locale: &Locale,
) -> String {
    match part {
        // Year formatting
        DatePart::Year2 => format!("{:02}", year % 100),
        DatePart::Year3 => format!("{:03}", year),
        DatePart::Year4 => format!("{:04}", year),

        // Buddhist calendar (Thai Buddhist Era)
        DatePart::BuddhistYear2 => {
            // Thai Buddhist calendar: Gregorian year + 543
            let buddhist_year = year + 543;
            format!("{:02}", buddhist_year % 100)
        }
        DatePart::BuddhistYear4 => {
            // Thai Buddhist calendar: Gregorian year + 543
            let buddhist_year = year + 543;
            format!("{:04}", buddhist_year)
        }
        DatePart::BuddhistYear4Alt => {
            // Hijri calendar (B2yyyy prefix)
            // Year has already been adjusted by fix_hijri conversion above
            // Just format the year as-is
            format!("{:04}", year)
        }
        DatePart::BuddhistYear2Alt => {
            // Hijri calendar (B2yy prefix)
            // Year has already been adjusted by fix_hijri conversion above
            // Just format last 2 digits
            format!("{:02}", year % 100)
        }

        // Month formatting
        DatePart::Month => format!("{}", month),
        DatePart::Month2 => format!("{:02}", month),
        DatePart::MonthAbbr => locale.month_names_short[(month - 1) as usize].to_string(),
        DatePart::MonthFull => locale.month_names_full[(month - 1) as usize].to_string(),
        DatePart::MonthLetter => {
            // First letter of the month name
            locale.month_names_full[(month - 1) as usize]
                .chars()
                .next()
                .unwrap_or('?')
                .to_string()
        }

        // Day formatting
        DatePart::Day => format!("{}", day),
        DatePart::Day2 => format!("{:02}", day),
        DatePart::DayAbbr => {
            // weekday is 1=Sunday...7=Saturday, array is 0-indexed
            locale.day_names_short[(weekday - 1) as usize].to_string()
        }
        DatePart::DayFull => locale.day_names_full[(weekday - 1) as usize].to_string(),

        // Hour formatting
        DatePart::Hour => {
            let h = if has_ampm { to_12_hour(hour) } else { hour };
            format!("{}", h)
        }
        DatePart::Hour2 => {
            let h = if has_ampm { to_12_hour(hour) } else { hour };
            format!("{:02}", h)
        }

        // Minute formatting
        DatePart::Minute => format!("{}", minute),
        DatePart::Minute2 => format!("{:02}", minute),

        // Second formatting
        DatePart::Second => format!("{}", second),
        DatePart::Second2 => format!("{:02}", second),

        // Sub-second formatting
        DatePart::SubSecond(places) => {
            // For sub-second precision, we need the fractional seconds from the original serial
            // Calculate total seconds with fractional part
            let time_fraction = serial.fract().abs();
            let total_seconds = time_fraction * 86400.0; // seconds in a day
            let subsecond_fraction = total_seconds.fract();

            if places == 0 {
                String::new()
            } else {
                let multiplier = 10_u32.pow(places as u32);
                // Round to high precision first to handle floating point errors
                let high_precision = (subsecond_fraction * 10000.0).round() / 10000.0;

                // Use different rounding strategies based on whether there are multiple subsecond displays
                let subsec = if has_multiple_subseconds {
                    // Multiple subsecond displays: truncate for consistency
                    (high_precision * multiplier as f64) as u32 % multiplier
                } else {
                    // Single subsecond display: round
                    ((high_precision * multiplier as f64).round() as u32) % multiplier
                };
                format!("{:0width$}", subsec, width = places as usize)
            }
        }
    }
}

/// Convert 24-hour time to 12-hour format.
/// 0 -> 12, 1-12 -> 1-12, 13-23 -> 1-11
fn to_12_hour(hour: u32) -> u32 {
    match hour {
        0 => 12,
        1..=12 => hour,
        _ => hour - 12,
    }
}

/// Format AM/PM indicator.
fn format_ampm(style: AmPmStyle, hour: u32, locale: &Locale) -> String {
    let is_pm = hour >= 12;

    // Excel always outputs uppercase AM/PM regardless of format case
    match style {
        AmPmStyle::Upper | AmPmStyle::Lower => {
            if is_pm {
                locale.pm_string.to_uppercase()
            } else {
                locale.am_string.to_uppercase()
            }
        }
        AmPmStyle::ShortUpper | AmPmStyle::ShortLower => {
            if is_pm {
                "P".to_string()
            } else {
                "A".to_string()
            }
        }
        AmPmStyle::MalformedUpper => {
            // Malformed AM/P pattern: outputs A0/P or A1/P
            // The '1' appears when 12-hour hour is 12 (noon or midnight)
            let hour_12 = to_12_hour(hour);
            let digit = if hour_12 == 12 { '1' } else { '0' };
            format!("A{}/P", digit)
        }
        AmPmStyle::MalformedLower => {
            // Malformed am/p pattern: outputs a0/p or a1/p
            let hour_12 = to_12_hour(hour);
            let digit = if hour_12 == 12 { '1' } else { '0' };
            format!("a{}/p", digit)
        }
    }
}

/// Apply pre-rounding to time components based on smallest displayed time unit.
/// Based on SSF's eval_fmt in bits/82_eval.js lines 102-115.
/// This ensures proper rounding when displaying limited time precision.
fn apply_time_prerounding(
    hour: &mut u32,
    minute: &mut u32,
    second: &mut u32,
    subseconds: f64,
    smallest_unit: crate::ast::TimeUnit,
    subsecond_precision: Option<u8>,
) {
    use crate::ast::TimeUnit;

    match smallest_unit {
        TimeUnit::Hours => {
            // Round subseconds -> seconds -> minutes -> hours
            let mut sec = *second as i64;
            let mut min = *minute as i64;
            let mut hr = *hour as i64;

            if subseconds >= 0.5 {
                sec += 1;
            }
            if sec >= 60 {
                sec = 0;
                min += 1;
            }
            if min >= 60 {
                min = 0;
                hr += 1;
            }
            if hr >= 24 {
                hr %= 24; // Wrap around if we overflow into next day
            }

            *hour = hr as u32;
            *minute = min as u32;
            *second = sec as u32;
        }
        TimeUnit::Minutes => {
            // Round subseconds -> seconds -> minutes (don't carry to hours)
            let mut sec = *second as i64;
            let mut min = *minute as i64;

            if subseconds >= 0.5 {
                sec += 1;
            }
            if sec >= 60 {
                sec = 0;
                min += 1;
            }
            if min >= 60 {
                min %= 60; // Wrap around if we overflow
            }

            *minute = min as u32;
            *second = sec as u32;
        }
        TimeUnit::Seconds => {
            // Round subseconds -> seconds (don't carry to minutes)
            let mut sec = *second as i64;

            if subseconds >= 0.5 {
                sec += 1;
            }
            if sec >= 60 {
                sec %= 60; // Wrap around if we overflow
            }

            *second = sec as u32;
        }
        TimeUnit::Subseconds => {
            // For subsecond display, check if the subseconds would round up to the next second
            // based on the display precision.
            // For n decimal places, threshold = 1 - 0.5 * 10^(-n)
            // e.g., .0 (1 place): 0.95 rounds to 1.0
            //       .00 (2 places): 0.995 rounds to 1.00
            if let Some(precision) = subsecond_precision {
                let threshold = 1.0 - 0.5 * 10_f64.powi(-(precision as i32));
                if subseconds >= threshold {
                    let mut sec = *second as i64 + 1;
                    if sec >= 60 {
                        sec %= 60;
                    }
                    *second = sec as u32;
                }
            }
        }
        TimeUnit::None => {
            // No rounding needed
        }
    }
}

/// Format elapsed time (total hours, minutes, or seconds).
fn format_elapsed(part: ElapsedPart, serial_value: f64) -> String {
    // SSF algorithm: parse serial into integer time components first, then calculate elapsed
    // This matches Excel's behavior exactly

    // Get integer and fractional parts
    let mut date = serial_value.floor() as i64;
    let frac = serial_value - date as f64;

    // Calculate total seconds in the fractional day, floored to integer
    let mut time_seconds = (86400.0 * frac).floor() as i64;

    // Calculate subsecond fractional part
    let mut subseconds = 86400.0 * frac - time_seconds as f64;

    // Handle subsecond carry-over (SSF does this at line 8-10 of 35_datecode.js)
    if subseconds > 0.9999 {
        subseconds = 0.0;
        time_seconds += 1;
        if time_seconds == 86400 {
            time_seconds = 0;
            date += 1;
        }
    }

    // Extract H, M, S components (all integers)
    let mut seconds = time_seconds % 60;
    let mut minutes = (time_seconds / 60) % 60;
    let mut hours = time_seconds / 3600;

    // SSF performs pre-rounding based on which time fields are present (lines 102-115 in 82_eval.js)
    // This ensures that when displaying [m], we round up if seconds would round to 60
    match part {
        ElapsedPart::Hours | ElapsedPart::Hours2 => {
            // For hours format: round subseconds, then carry over through S -> M -> H
            if subseconds >= 0.5 {
                seconds += 1;
            }
            if seconds >= 60 {
                // seconds = 0; (not needed, variable unused after)
                minutes += 1;
            }
            if minutes >= 60 {
                // minutes = 0; (not needed, variable unused after)
                hours += 1;
            }
            // Total elapsed hours: D*24 + H (all integer arithmetic after rounding)
            let total_hours = date * 24 + hours;
            if matches!(part, ElapsedPart::Hours2) {
                format!("{:02}", total_hours)
            } else {
                format!("{}", total_hours)
            }
        }
        ElapsedPart::Minutes | ElapsedPart::Minutes2 => {
            // For minutes format: round subseconds, then carry over S -> M (not to H)
            if subseconds >= 0.5 {
                seconds += 1;
            }
            if seconds >= 60 {
                // seconds = 0; (not needed, variable unused after)
                minutes += 1;
            }
            // Total elapsed minutes: (D*24+H)*60 + M (all integer arithmetic after rounding)
            let total_minutes = (date * 24 + hours) * 60 + minutes;
            if matches!(part, ElapsedPart::Minutes2) {
                format!("{:02}", total_minutes)
            } else {
                format!("{}", total_minutes)
            }
        }
        ElapsedPart::Seconds | ElapsedPart::Seconds2 => {
            // For seconds format: round S+u directly, no pre-rounding
            // Total elapsed seconds: ((D*24+H)*60+M)*60 + round(S+u)
            let total_seconds = ((date * 24 + hours) * 60 + minutes) * 60 + (seconds as f64 + subseconds).round() as i64;
            if matches!(part, ElapsedPart::Seconds2) {
                format!("{:02}", total_seconds)
            } else {
                format!("{}", total_seconds)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_12_hour() {
        assert_eq!(to_12_hour(0), 12);
        assert_eq!(to_12_hour(1), 1);
        assert_eq!(to_12_hour(11), 11);
        assert_eq!(to_12_hour(12), 12);
        assert_eq!(to_12_hour(13), 1);
        assert_eq!(to_12_hour(23), 11);
    }
}
