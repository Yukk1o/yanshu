#![forbid(unsafe_code)]

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

use super::{DecimalIssue, MAXIMUM_DECIMAL_INTEGER_BITS, MAXIMUM_DECIMAL_SCALE, checked_scale};

pub(super) fn rescale(
    value: &BigInt,
    from_scale: &BigInt,
    to_scale: &BigInt,
    rounding: &str,
) -> Result<BigInt, DecimalIssue> {
    let from_scale = checked_scale(from_scale)?;
    let to_scale = checked_scale(to_scale)?;
    let rounding = Rounding::parse(rounding)?;
    if value.bits() > MAXIMUM_DECIMAL_INTEGER_BITS {
        return Err(integer_limit());
    }
    match to_scale.cmp(&from_scale) {
        Ordering::Equal => Ok(value.clone()),
        Ordering::Greater => upscale(value, to_scale.saturating_sub(from_scale)),
        Ordering::Less => downscale(value, from_scale.saturating_sub(to_scale), rounding),
    }
}

fn upscale(value: &BigInt, delta: usize) -> Result<BigInt, DecimalIssue> {
    if value.is_zero() {
        return Ok(BigInt::ZERO);
    }
    let factor = power_of_ten(delta)?;
    let minimum_product_bits = value.bits().saturating_add(factor.bits()).saturating_sub(1);
    if minimum_product_bits > MAXIMUM_DECIMAL_INTEGER_BITS {
        return Err(integer_limit());
    }
    let result = value * factor;
    if result.bits() > MAXIMUM_DECIMAL_INTEGER_BITS {
        Err(integer_limit())
    } else {
        Ok(result)
    }
}

fn downscale(value: &BigInt, delta: usize, rounding: Rounding) -> Result<BigInt, DecimalIssue> {
    let divisor = power_of_ten(delta)?;
    let quotient = value / &divisor;
    let remainder = value % &divisor;
    if remainder.is_zero() {
        return Ok(quotient);
    }
    let away = || {
        if value.is_negative() {
            &quotient - 1_u8
        } else {
            &quotient + 1_u8
        }
    };
    match rounding {
        Rounding::Exact => Err(DecimalIssue::simple("DECIMAL_ROUNDING_REQUIRED")),
        Rounding::TowardZero => Ok(quotient),
        Rounding::Floor if value.is_negative() => Ok(&quotient - 1_u8),
        Rounding::Floor => Ok(quotient),
        Rounding::Ceiling if value.is_positive() => Ok(&quotient + 1_u8),
        Rounding::Ceiling => Ok(quotient),
        Rounding::HalfUp | Rounding::HalfEven => {
            let comparison = (remainder.abs() * 2_u8).cmp(&divisor);
            if comparison == Ordering::Greater
                || (comparison == Ordering::Equal
                    && (rounding == Rounding::HalfUp || !(&quotient % 2_u8).is_zero()))
            {
                Ok(away())
            } else {
                Ok(quotient)
            }
        }
    }
}

fn power_of_ten(exponent: usize) -> Result<BigInt, DecimalIssue> {
    let exponent = u32::try_from(exponent)
        .map_err(|_| DecimalIssue::limit("DECIMAL_SCALE_LIMIT", MAXIMUM_DECIMAL_SCALE))?;
    Ok(BigInt::from(10_u8).pow(exponent))
}

fn integer_limit() -> DecimalIssue {
    DecimalIssue::limit("DECIMAL_INTEGER_LIMIT", MAXIMUM_DECIMAL_INTEGER_BITS)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rounding {
    Exact,
    TowardZero,
    Floor,
    Ceiling,
    HalfUp,
    HalfEven,
}

impl Rounding {
    fn parse(value: &str) -> Result<Self, DecimalIssue> {
        match value {
            "exact" => Ok(Self::Exact),
            "toward-zero" => Ok(Self::TowardZero),
            "floor" => Ok(Self::Floor),
            "ceiling" => Ok(Self::Ceiling),
            "half-up" => Ok(Self::HalfUp),
            "half-even" => Ok(Self::HalfEven),
            _ => Err(DecimalIssue::simple("DECIMAL_INVALID_ROUNDING_MODE")),
        }
    }
}
