//! Exact decimal amounts for number goals.
//!
//! A [`Quantity`] is a whole number of ten-thousandths, so sums and pace
//! comparisons are exact: logging 0.1 km ten times makes exactly 1 km, and a
//! check-in that lands exactly on the pace line reads on track. Four decimal
//! places cover the amounts goals are measured in (cents, metres within a
//! kilometre, tens of grams within a kilogram) with room to spare; an amount
//! with more is refused rather than rounded, so what is stored is what was
//! typed.

use std::fmt;
use std::str::FromStr;

use crate::error::GoalsError;

/// Decimal places a [`Quantity`] keeps.
pub const DECIMALS: u32 = 4;
const SCALE: i64 = 10_i64.pow(DECIMALS);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Quantity(i64);

impl Quantity {
    pub const ZERO: Quantity = Quantity(0);

    /// The amount `units / 10^DECIMALS`.
    pub const fn from_units(units: i64) -> Self {
        Quantity(units)
    }

    /// Whole ten-thousandths, as stored.
    pub const fn units(self) -> i64 {
        self.0
    }

    pub fn whole(n: i64) -> Option<Self> {
        n.checked_mul(SCALE).map(Quantity)
    }
}

impl FromStr for Quantity {
    type Err = GoalsError;

    /// `5`, `-2.5`, `+0.125`, `.5`. No exponents, separators or more than
    /// [`DECIMALS`] decimal places.
    fn from_str(s: &str) -> Result<Self, GoalsError> {
        let bad = || GoalsError::Validation(format!("{s:?} is not an amount"));
        let text = s.trim();
        let (negative, digits) = match text.as_bytes().first() {
            Some(b'-') => (true, &text[1..]),
            Some(b'+') => (false, &text[1..]),
            _ => (false, text),
        };
        let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
        let all_digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        if (whole.is_empty() && fraction.is_empty()) || !all_digits(whole) || !all_digits(fraction)
        {
            return Err(bad());
        }
        if fraction.len() > DECIMALS as usize {
            return Err(GoalsError::Validation(format!(
                "{s:?} has more than {DECIMALS} decimal places"
            )));
        }
        let too_large = || GoalsError::Validation(format!("{s:?} is too large"));
        let whole: i64 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| too_large())?
        };
        let mut fraction_units: i64 = 0;
        for (i, b) in fraction.bytes().enumerate() {
            fraction_units += i64::from(b - b'0') * 10_i64.pow(DECIMALS - 1 - i as u32);
        }
        let units = whole
            .checked_mul(SCALE)
            .and_then(|u| u.checked_add(fraction_units))
            .ok_or_else(too_large)?;
        Ok(Quantity(if negative { -units } else { units }))
    }
}

impl fmt::Display for Quantity {
    /// The shortest exact form: `5`, `-2.5`, `0.125`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let scale = SCALE as u64;
        let (whole, fraction) = (abs / scale, abs % scale);
        if fraction == 0 {
            return write!(f, "{sign}{whole}");
        }
        let digits = format!("{fraction:0width$}", width = DECIMALS as usize);
        write!(f, "{sign}{whole}.{}", digits.trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(s: &str) -> Quantity {
        s.parse().unwrap()
    }

    #[test]
    fn parses_and_prints_exactly() {
        for (text, units, shown) in [
            ("5", 50_000, "5"),
            ("-2.5", -25_000, "-2.5"),
            ("+0.125", 1_250, "0.125"),
            (".5", 5_000, "0.5"),
            ("7.", 70_000, "7"),
            ("0.0001", 1, "0.0001"),
            ("-0.0001", -1, "-0.0001"),
            (" 10000 ", 100_000_000, "10000"),
            ("0", 0, "0"),
            ("-0", 0, "0"),
        ] {
            assert_eq!(q(text).units(), units, "{text}");
            assert_eq!(q(text).to_string(), shown, "{text}");
        }
    }

    #[test]
    fn refuses_what_it_cannot_store_exactly() {
        for text in [
            "", "-", ".", "1.2.3", "1e3", "1,000", "abc", "0.00001", "--1", "1 000",
        ] {
            assert!(text.parse::<Quantity>().is_err(), "{text:?}");
        }
        let largest = (i64::MAX / SCALE).to_string();
        assert!(largest.parse::<Quantity>().is_ok());
        assert!(format!("{}", i64::MAX / SCALE + 1)
            .parse::<Quantity>()
            .is_err());
    }

    #[test]
    fn tenths_add_up_exactly() {
        let tenth = q("0.1");
        let sum = (0..10).fold(0, |acc, _| acc + tenth.units());
        assert_eq!(Quantity::from_units(sum), q("1"));
    }
}
