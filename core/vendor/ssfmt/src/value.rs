//! Value types that can be formatted.

/// A value that can be formatted using a number format code.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// A numeric value (including Excel serial dates)
    Number(f64),
    /// A text value
    Text(&'a str),
    /// A boolean value
    Bool(bool),
    /// An empty cell
    Empty,
    /// An arbitrary-precision integer (requires `bigint` feature)
    /// Use this for integers larger than 2^53 that would lose precision as f64.
    #[cfg(feature = "bigint")]
    BigInt(num_bigint::BigInt),
    /// A chrono DateTime (requires `chrono` feature)
    #[cfg(feature = "chrono")]
    DateTime(chrono::NaiveDateTime),
    /// A chrono Date (requires `chrono` feature)
    #[cfg(feature = "chrono")]
    Date(chrono::NaiveDate),
    /// A chrono Time (requires `chrono` feature)
    #[cfg(feature = "chrono")]
    Time(chrono::NaiveTime),
}

impl<'a> From<f64> for Value<'a> {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl<'a> From<f32> for Value<'a> {
    fn from(n: f32) -> Self {
        Value::Number(n as f64)
    }
}

impl<'a> From<i64> for Value<'a> {
    fn from(n: i64) -> Self {
        Value::Number(n as f64)
    }
}

impl<'a> From<i32> for Value<'a> {
    fn from(n: i32) -> Self {
        Value::Number(n as f64)
    }
}

impl<'a> From<&'a str> for Value<'a> {
    fn from(s: &'a str) -> Self {
        Value::Text(s)
    }
}

impl<'a> From<bool> for Value<'a> {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl<'a> From<()> for Value<'a> {
    fn from(_: ()) -> Self {
        Value::Empty
    }
}

#[cfg(feature = "bigint")]
impl<'a> From<num_bigint::BigInt> for Value<'a> {
    fn from(n: num_bigint::BigInt) -> Self {
        Value::BigInt(n)
    }
}

#[cfg(feature = "bigint")]
impl<'a> From<i128> for Value<'a> {
    fn from(n: i128) -> Self {
        Value::BigInt(num_bigint::BigInt::from(n))
    }
}

#[cfg(feature = "bigint")]
impl<'a> From<u128> for Value<'a> {
    fn from(n: u128) -> Self {
        Value::BigInt(num_bigint::BigInt::from(n))
    }
}

#[cfg(feature = "chrono")]
impl<'a> From<chrono::NaiveDateTime> for Value<'a> {
    fn from(dt: chrono::NaiveDateTime) -> Self {
        Value::DateTime(dt)
    }
}

#[cfg(feature = "chrono")]
impl<'a> From<chrono::NaiveDate> for Value<'a> {
    fn from(d: chrono::NaiveDate) -> Self {
        Value::Date(d)
    }
}

#[cfg(feature = "chrono")]
impl<'a> From<chrono::NaiveTime> for Value<'a> {
    fn from(t: chrono::NaiveTime) -> Self {
        Value::Time(t)
    }
}

impl<'a> Value<'a> {
    /// Returns the value as a number if possible.
    /// For BigInt values, returns the f64 representation (may lose precision for large values).
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(true) => Some(1.0),
            Value::Bool(false) => Some(0.0),
            #[cfg(feature = "bigint")]
            Value::BigInt(n) => {
                // Convert to f64 - may lose precision for very large values
                // For safe range checking, use is_safe_integer() first
                let float_val = n.to_string().parse::<f64>().unwrap_or(f64::NAN);
                Some(float_val)
            }
            _ => None,
        }
    }

    /// Returns the value as text if it is text.
    pub fn as_text(&self) -> Option<&'a str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns true if this value is empty.
    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    /// Returns a type name for error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Text(_) => "text",
            Value::Bool(_) => "boolean",
            Value::Empty => "empty",
            #[cfg(feature = "bigint")]
            Value::BigInt(_) => "bigint",
            #[cfg(feature = "chrono")]
            Value::DateTime(_) => "datetime",
            #[cfg(feature = "chrono")]
            Value::Date(_) => "date",
            #[cfg(feature = "chrono")]
            Value::Time(_) => "time",
        }
    }

    /// Returns true if this is a BigInt value.
    #[cfg(feature = "bigint")]
    pub fn is_bigint(&self) -> bool {
        matches!(self, Value::BigInt(_))
    }

    /// Returns true if this BigInt value is within the safe integer range for f64 (±2^53).
    /// For non-BigInt values, always returns true.
    #[cfg(feature = "bigint")]
    pub fn is_safe_integer(&self) -> bool {
        match self {
            Value::BigInt(n) => {
                use num_bigint::BigInt;
                // Safe integer range: -9007199254740991 to 9007199254740991 (±(2^53 - 1))
                let min_safe = BigInt::from(-9_007_199_254_740_991_i64);
                let max_safe = BigInt::from(9_007_199_254_740_991_i64);
                n >= &min_safe && n <= &max_safe
            }
            _ => true,
        }
    }

    /// Returns the BigInt value if this is a BigInt.
    #[cfg(feature = "bigint")]
    pub fn as_bigint(&self) -> Option<&num_bigint::BigInt> {
        match self {
            Value::BigInt(n) => Some(n),
            _ => None,
        }
    }
}
