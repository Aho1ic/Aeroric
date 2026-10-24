//! Formatting options and configuration.

use crate::locale::Locale;

/// The date system used for serial number conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateSystem {
    /// Windows Excel default (1900-based, includes leap year bug)
    #[default]
    Date1900,
    /// Mac Excel legacy (1904-based)
    Date1904,
}

impl DateSystem {
    /// Returns the epoch year for this date system.
    pub fn epoch_year(&self) -> i32 {
        match self {
            DateSystem::Date1900 => 1900,
            DateSystem::Date1904 => 1904,
        }
    }
}

/// Options for formatting values.
#[derive(Debug, Clone, Default)]
pub struct FormatOptions {
    /// The date system to use for serial number conversion.
    pub date_system: DateSystem,
    /// The locale for formatting.
    pub locale: Locale,
}
