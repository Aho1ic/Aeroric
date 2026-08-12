//! Built-in locale data.

/// Locale settings for formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locale {
    pub decimal_separator: char,
    pub thousands_separator: char,
    pub currency_symbol: &'static str,
    pub am_string: &'static str,
    pub pm_string: &'static str,
    pub month_names_short: [&'static str; 12],
    pub month_names_full: [&'static str; 12],
    pub day_names_short: [&'static str; 7],
    pub day_names_full: [&'static str; 7],
}

impl Default for Locale {
    fn default() -> Self {
        Self::en_us()
    }
}

impl Locale {
    /// US English locale.
    pub fn en_us() -> Self {
        Locale {
            decimal_separator: '.',
            thousands_separator: ',',
            currency_symbol: "$",
            am_string: "AM",
            pm_string: "PM",
            month_names_short: [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ],
            month_names_full: [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
            day_names_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
            day_names_full: [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ],
        }
    }
}
