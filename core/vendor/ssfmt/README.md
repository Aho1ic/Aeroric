# ssfmt

Excel-compatible ECMA-376 number format codes for Rust.

## Features

- Parse and format Excel/OOXML number format codes
- Match Excel's actual behavior, including quirks
- Support for dates, times, percentages, fractions
- Multiple format sections (positive/negative/zero/text)
- Color and conditional format detection
- Both 1900 and 1904 date systems
- Efficient compile-once, format-many pattern

## Usage

```rust
use ssfmt::{format_default, NumberFormat, FormatOptions};

// Simple one-off formatting
let result = format_default(1234.56, "#,##0.00").unwrap();
assert_eq!(result, "1,234.56");

// Compile once, format many values
let fmt = NumberFormat::parse("yyyy-mm-dd").unwrap();
let opts = FormatOptions::default();
assert_eq!(fmt.format(46031.0, &opts), "2026-01-09");
```

## Compatibility

Achieves **99.9999% compatibility** with SheetJS SSF across 19.5+ million test cases.

See [docs/TESTING.md](docs/TESTING.md) for detailed test coverage.

## Documentation

- [Architecture Guide](docs/ARCHITECTURE.md) - Design principles and key components
- [Test Coverage](docs/TESTING.md) - Comprehensive testing documentation
- [Implementation Plans](docs/plans/) - Original design and implementation notes

## License

MIT OR Apache-2.0
