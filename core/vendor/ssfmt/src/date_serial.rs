//! Date serial number conversion utilities
//!
//! Excel stores dates as serial numbers representing days since a base date:
//! - 1900 system: Day 1 = January 1, 1900 (Windows default)
//! - 1904 system: Day 1 = January 2, 1904 (Mac legacy)
//!
//! The 1900 system includes the infamous leap year bug: Excel treats 1900 as a
//! leap year (it wasn't), so day 60 is February 29, 1900 (which didn't exist).
//! Days after 60 are effectively shifted by 1 to compensate.
//!
//! Time is stored as the fractional part of the serial number:
//! - 0.5 = 12:00:00 (noon)
//! - 0.75 = 18:00:00 (6 PM)
//!
//! # Performance
//!
//! This module uses O(1) algorithms for date conversion based on Julian Day
//! Number formulas (Fliegel & Van Flandern, 1968) instead of iterating through
//! years/months. This provides consistent performance regardless of the date.

use crate::options::DateSystem;

/// Convert an Excel serial number to a date (year, month, day).
///
/// Returns `None` if the serial number is invalid (negative or zero for some systems).
///
/// # Arguments
/// * `serial` - The Excel serial number (integer part is the date)
/// * `system` - The date system to use
///
/// # Returns
/// * `Some((year, month, day))` on success
/// * `None` if the serial number is out of range
///
/// # Excel's Leap Year Bug
/// In the 1900 system, day 60 returns (1900, 2, 29) even though February 29, 1900
/// didn't actually exist. This matches Excel's behavior.
pub fn serial_to_date(serial: f64, system: DateSystem) -> Option<(i32, u32, u32)> {
    let days = serial.floor() as i64;

    if days < 1 {
        return None;
    }

    match system {
        DateSystem::Date1900 => serial_to_date_1900(days),
        DateSystem::Date1904 => serial_to_date_1904(days),
    }
}

/// Convert serial number to date using the 1900 system.
///
/// Uses an O(1) algorithm based on Julian Day Number conversion
/// (Fliegel & Van Flandern, 1968) instead of iterating through years.
fn serial_to_date_1900(days: i64) -> Option<(i32, u32, u32)> {
    // Handle Excel's leap year bug: day 60 = Feb 29, 1900 (doesn't exist)
    if days == 60 {
        return Some((1900, 2, 29));
    }

    // Handle early 1900 dates specially (days 1-59)
    // These are before the leap year bug kicks in
    if days < 60 {
        // Day 1 = Jan 1, 1900
        if days < 32 {
            return Some((1900, 1, days as u32));
        } else {
            return Some((1900, 2, (days - 31) as u32));
        }
    }

    // For days > 60, use the O(1) Julian Day Number algorithm
    // This converts Excel serial to Gregorian date.
    //
    // The algorithm is based on Fliegel & Van Flandern (1968).
    // We need to account for the Excel leap year bug: day 60 is the phantom
    // Feb 29, 1900, so days > 60 are shifted by 1 compared to the real calendar.
    //
    // The constant 2_415_019 = JDN for Dec 31, 1899 (Excel day 0)
    // By not subtracting 1 for the leap year bug, we effectively treat
    // the Excel serial as if Excel's calendar were correct (which it isn't,
    // but matches what Excel displays).
    let ord = days;

    // Convert Excel serial to Julian Day Number, then to Gregorian
    let mut l = ord + 68_569 + 2_415_019;
    let n = (4 * l) / 146_097;
    l -= (146_097 * n + 3) / 4;
    let i = (4_000 * (l + 1)) / 1_461_001;
    l = l - ((1_461 * i) / 4) + 31;
    let j = (80 * l) / 2_447;
    let n_day = l - ((2_447 * j) / 80);
    l = j / 11;
    let n_month = j + 2 - (12 * l);
    let n_year = 100 * (n - 49) + i + l;

    Some((n_year as i32, n_month as u32, n_day as u32))
}

/// Convert serial number to date using the 1904 system.
///
/// Uses O(1) algorithm by converting to 1900 system equivalent.
fn serial_to_date_1904(days: i64) -> Option<(i32, u32, u32)> {
    // The 1904 system is offset from 1900 by 1462 days
    // Day 1 in 1904 system = Jan 2, 1904 = Day 1463 in 1900 system
    // We add 1462 to convert to 1900 system, then use the O(1) algorithm
    serial_to_date_1900(days + 1462)
}

/// Extract the time components (hours, minutes, seconds) from a serial number.
///
/// The time is the fractional part of the serial number:
/// - 0.0 = 00:00:00
/// - 0.5 = 12:00:00
/// - 0.75 = 18:00:00
///
/// # Arguments
/// * `serial` - The Excel serial number (fractional part is the time)
///
/// # Returns
/// * `(hours, minutes, seconds)` where hours is 0-23, minutes and seconds are 0-59
pub fn serial_to_time(serial: f64) -> (u32, u32, u32) {
    serial_to_time_impl(serial, true)
}

/// Convert Excel serial number to time components (hour, minute, second).
/// The `round_seconds` parameter controls whether to round fractional seconds.
/// Set to false when the format includes subsecond display (.0, .00, etc.).
pub fn serial_to_time_with_rounding(serial: f64, round_seconds: bool) -> (u32, u32, u32) {
    serial_to_time_impl(serial, round_seconds)
}

fn serial_to_time_impl(serial: f64, round_seconds: bool) -> (u32, u32, u32) {
    // Get the fractional part (time component)
    let fraction = serial.fract().abs();

    // Convert to total seconds in a day (86400 seconds)
    let total_seconds = if round_seconds {
        // Round to handle fractional seconds close to the next second
        // Excel rounds seconds when displaying time without subseconds
        (fraction * 86400.0).round() as u32
    } else {
        // For subsecond display, round to millisecond precision first to handle
        // floating point errors (e.g., 0.7 is stored as 0.69999... in f64),
        // then truncate to get the integer seconds.
        // This ensures 0.7 displays as 16:48:00.000 not 16:47:59.999
        let total_with_subseconds = (fraction * 86400.0 * 1000.0).round() / 1000.0;
        total_with_subseconds as u32
    };

    let hours = (total_seconds / 3600) % 24;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    (hours, minutes, seconds)
}

/// Convert a date (year, month, day) to an Excel serial number.
///
/// Uses an O(1) algorithm based on Julian Day Number conversion.
///
/// # Arguments
/// * `year` - The year (e.g., 2026)
/// * `month` - The month (1-12)
/// * `day` - The day of month (1-31)
/// * `system` - The date system to use
///
/// # Returns
/// The Excel serial number for the given date
pub fn date_to_serial(year: i32, month: u32, day: u32, system: DateSystem) -> f64 {
    match system {
        DateSystem::Date1900 => date_to_serial_1900(year, month, day),
        DateSystem::Date1904 => date_to_serial_1904(year, month, day),
    }
}

/// Convert date to serial using the 1900 system.
///
/// Uses an O(1) algorithm based on the civil date formula.
fn date_to_serial_1900(year: i32, month: u32, day: u32) -> f64 {
    // Special case for the phantom Feb 29, 1900
    if year == 1900 && month == 2 && day == 29 {
        return 60.0;
    }

    // Use O(1) algorithm to convert Gregorian to days since epoch
    // Based on Howard Hinnant's date algorithms
    // http://howardhinnant.github.io/date_algorithms.html
    let y = year - (month <= 2) as i32;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32; // year of era [0, 399]
    let m = month as i32;
    let d = day as i32;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1; // day of year [0, 365]
    let doe = yoe as i32 * 365 + yoe as i32 / 4 - yoe as i32 / 100 + doy; // day of era [0, 146096]

    // Days since epoch (March 1, 0000 in proleptic Gregorian)
    let days_since_epoch = era as i64 * 146_097 + doe as i64 - 719_468;

    // Convert to Excel serial (Excel day 1 = Jan 1, 1900)
    // Jan 1, 1900 = days_since_epoch of -25567
    // So Excel serial = days_since_epoch + 25568
    let mut serial = days_since_epoch + 25568;

    // Add 1 for the leap year bug (for dates after Feb 28, 1900)
    if serial >= 60 {
        serial += 1;
    }

    serial as f64
}

/// Convert date to serial using the 1904 system.
///
/// Uses O(1) algorithm by calculating the 1900 equivalent and adjusting.
fn date_to_serial_1904(year: i32, month: u32, day: u32) -> f64 {
    // Get the 1900 system serial and subtract the offset
    // Day 1 in 1904 system = Jan 2, 1904 = Day 1463 in 1900 system
    date_to_serial_1900(year, month, day) - 1462.0
}

/// Get the day of the week from a serial number.
///
/// # Arguments
/// * `serial` - The Excel serial number
/// * `system` - The date system to use
///
/// # Returns
/// Day of week: 1 = Sunday, 2 = Monday, ..., 7 = Saturday
/// (matches Excel's WEEKDAY function with return_type=1)
pub fn serial_to_weekday(serial: f64, system: DateSystem) -> u32 {
    let days = serial.floor() as i64;

    match system {
        DateSystem::Date1900 => {
            // Day 1 (Jan 1, 1900) was a Sunday (day 1)
            // Day 0 (Dec 31, 1899) was a Saturday (day 7)
            // Use proper modulo to handle negative numbers correctly
            let weekday = ((days - 1) % 7 + 7) % 7 + 1;
            weekday as u32
        }
        DateSystem::Date1904 => {
            // Day 0 (Jan 1, 1904) was a Friday
            // Day 1 (Jan 2, 1904) was a Saturday
            let adjusted = (days + 5) % 7 + 1; // +5 because Friday=6, and we want Sunday=1
            adjusted as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serial_to_date_1900_early() {
        // Day 1 = Jan 1, 1900
        assert_eq!(
            serial_to_date(1.0, DateSystem::Date1900),
            Some((1900, 1, 1))
        );
        // Day 2 = Jan 2, 1900
        assert_eq!(
            serial_to_date(2.0, DateSystem::Date1900),
            Some((1900, 1, 2))
        );
        // Day 31 = Jan 31, 1900
        assert_eq!(
            serial_to_date(31.0, DateSystem::Date1900),
            Some((1900, 1, 31))
        );
        // Day 32 = Feb 1, 1900
        assert_eq!(
            serial_to_date(32.0, DateSystem::Date1900),
            Some((1900, 2, 1))
        );
    }

    #[test]
    fn test_serial_to_date_leap_year_bug() {
        // Day 59 = Feb 28, 1900
        assert_eq!(
            serial_to_date(59.0, DateSystem::Date1900),
            Some((1900, 2, 28))
        );
        // Day 60 = Feb 29, 1900 (Excel's bug - this date doesn't exist)
        assert_eq!(
            serial_to_date(60.0, DateSystem::Date1900),
            Some((1900, 2, 29))
        );
        // Day 61 = Mar 1, 1900
        assert_eq!(
            serial_to_date(61.0, DateSystem::Date1900),
            Some((1900, 3, 1))
        );
    }

    #[test]
    fn test_roundtrip_1900() {
        // Test roundtrip for various dates
        for &(y, m, d) in &[
            (1900, 1, 1),
            (1900, 3, 1),
            (2000, 2, 29), // Leap year
            (2024, 12, 31),
            (2026, 1, 9),
        ] {
            let serial = date_to_serial(y, m, d, DateSystem::Date1900);
            let (y2, m2, d2) = serial_to_date(serial, DateSystem::Date1900).unwrap();
            assert_eq!(
                (y, m, d),
                (y2, m2, d2),
                "Roundtrip failed for {}-{}-{} (serial={})",
                y,
                m,
                d,
                serial
            );
        }
    }

    #[test]
    fn test_serial_to_date_modern_dates() {
        // Test some modern dates to verify the O(1) algorithm
        // These values are verified against the SSF test suite which passes 100%
        // Excel serial 45000 = March 15, 2023 (verified via SSF tests)
        assert_eq!(
            serial_to_date(45000.0, DateSystem::Date1900),
            Some((2023, 3, 15))
        );
        // Excel serial 44197 = January 1, 2021
        assert_eq!(
            serial_to_date(44197.0, DateSystem::Date1900),
            Some((2021, 1, 1))
        );
        // Excel serial 43831 = January 1, 2020
        assert_eq!(
            serial_to_date(43831.0, DateSystem::Date1900),
            Some((2020, 1, 1))
        );
    }

    #[test]
    fn test_roundtrip_1904() {
        // Test roundtrip for 1904 system
        for &(y, m, d) in &[
            (1904, 1, 2), // Day 1
            (1904, 2, 29), // Leap year
            (2024, 12, 31),
        ] {
            let serial = date_to_serial(y, m, d, DateSystem::Date1904);
            let (y2, m2, d2) = serial_to_date(serial, DateSystem::Date1904).unwrap();
            assert_eq!(
                (y, m, d),
                (y2, m2, d2),
                "Roundtrip failed for {}-{}-{} (1904 system, serial={})",
                y,
                m,
                d,
                serial
            );
        }
    }

    #[test]
    fn test_date_to_serial_known_values() {
        // Test known date-to-serial conversions
        assert_eq!(date_to_serial(1900, 1, 1, DateSystem::Date1900), 1.0);
        assert_eq!(date_to_serial(1900, 2, 28, DateSystem::Date1900), 59.0);
        assert_eq!(date_to_serial(1900, 2, 29, DateSystem::Date1900), 60.0); // Phantom leap day
        assert_eq!(date_to_serial(1900, 3, 1, DateSystem::Date1900), 61.0);
        assert_eq!(date_to_serial(2020, 1, 1, DateSystem::Date1900), 43831.0);
        assert_eq!(date_to_serial(2021, 1, 1, DateSystem::Date1900), 44197.0);
    }
}
