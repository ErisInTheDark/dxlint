//! Language-neutral number primitives: radix and exact integer text, the
//! decimal digits of a double, and 32-bit integer arithmetic. How a
//! language reads, prints and combines numbers is declared in `.dx` on top.

use std::cmp::Ordering;

/// A number written in `radix`: 10 reads decimal notation (sign, digits,
/// fraction, exponent; nothing else), other radixes 2..=36 read whole digits
/// exactly, rounded to the nearest double.
pub fn parse_number(text: &str, radix: u32) -> Option<f64> {
    if radix == 10 {
        let shape = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E'));
        return if shape { text.parse().ok() } else { None };
    }
    big_integer(text, radix)?.parse().ok()
}

/// Whole `digits` in `radix` (2..=36, any case) as exact decimal text.
pub fn big_integer(digits: &str, radix: u32) -> Option<String> {
    if !(2..=36).contains(&radix) || digits.is_empty() { return None; }
    // Little-endian base 10^9 limbs.
    let mut limbs: Vec<u64> = vec![0];
    for c in digits.chars() {
        let mut carry = u64::from(c.to_digit(radix)?);
        for limb in &mut limbs {
            let v = *limb * u64::from(radix) + carry;
            *limb = v % 1_000_000_000;
            carry = v / 1_000_000_000;
        }
        if carry > 0 { limbs.push(carry); }
    }
    let mut out = limbs.last().copied().unwrap_or(0).to_string();
    for limb in limbs.iter().rev().skip(1) {
        out.push_str(&format!("{limb:09}"));
    }
    Some(out)
}

/// The order of two whole numbers written in decimal, each with an
/// optional `-`, exactly.
pub fn compare_integers(a: &str, b: &str) -> Option<Ordering> {
    fn parts(text: &str) -> Option<(bool, &str)> {
        let (negative, digits) = match text.strip_prefix('-') { Some(rest) => (true, rest), None => (false, text) };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
        let digits = digits.trim_start_matches('0');
        Some((negative && !digits.is_empty(), digits))
    }
    let ((a_negative, a), (b_negative, b)) = (parts(a)?, parts(b)?);
    let magnitude = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
    Some(match (a_negative, b_negative) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => magnitude.reverse(),
        (false, false) => magnitude,
    })
}

/// A positive finite `value`'s significant decimal digits and the exponent
/// of the first (`value` = d.ddd × 10^exponent): the shortest that reads
/// back as `value`, or `precision` digits rounding exact ties up.
pub fn digits(value: f64, precision: Option<usize>) -> Option<(String, i32)> {
    if !value.is_finite() || value <= 0.0 { return None; }
    let Some(precision) = precision else {
        let scientific = format!("{value:e}");
        let (mantissa, exponent) = scientific.split_once('e')?;
        return Some((mantissa.chars().filter(char::is_ascii_digit).collect(), exponent.parse().ok()?));
    };
    if !(1..=100).contains(&precision) { return None; }
    // A double's exact decimal expansion has at most 767 significant digits.
    let exact = format!("{value:.800e}");
    let (coefficient, exponent) = exact.split_once('e')?;
    let mut exponent: i32 = exponent.parse().ok()?;
    let all: Vec<u8> = coefficient.bytes().filter(u8::is_ascii_digit).collect();
    let mut kept = all[..precision].to_vec();
    if all[precision] >= b'5' {
        let mut at = precision;
        loop {
            if at == 0 {
                kept.insert(0, b'1');
                kept.pop();
                exponent += 1;
                break;
            }
            at -= 1;
            if kept[at] == b'9' { kept[at] = b'0'; } else { kept[at] += 1; break; }
        }
    }
    Some((String::from_utf8(kept).ok()?, exponent))
}

/// A whole finite `value` in `radix` (2..=36), lowercase; `None` for
/// fractions, infinities and other radixes.
pub fn integer_radix(value: f64, radix: u32) -> Option<String> {
    if !(2..=36).contains(&radix) || !value.is_finite() || value.fract() != 0.0 { return None; }
    if value == 0.0 { return Some("0".into()); }
    let bits = value.abs().to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1 << 52) - 1);
    let mantissa = if biased == 0 { fraction } else { fraction | (1 << 52) };
    let shift = biased.max(1) - 1075;
    let mut limbs: Vec<u32> = if shift >= 0 {
        let mut limbs = vec![0u32; shift as usize / 32];
        let low = u128::from(mantissa) << (shift % 32);
        limbs.extend([low as u32, (low >> 32) as u32, (low >> 64) as u32]);
        limbs
    } else {
        // A whole value drops only zero bits here.
        let whole = mantissa >> -shift;
        vec![whole as u32, (whole >> 32) as u32]
    };
    let mut text = Vec::new();
    while limbs.iter().any(|limb| *limb != 0) {
        let mut remainder = 0u64;
        for limb in limbs.iter_mut().rev() {
            let current = remainder << 32 | u64::from(*limb);
            *limb = (current / u64::from(radix)) as u32;
            remainder = current % u64::from(radix);
        }
        text.push(std::char::from_digit(remainder as u32, radix).expect("digit below radix"));
    }
    if value < 0.0 { text.push('-'); }
    Some(text.into_iter().rev().collect())
}

/// `value` truncated and wrapped into a signed 32-bit integer (0 for
/// non-finite values).
pub fn int32(value: f64) -> i32 {
    if !value.is_finite() { return 0; }
    value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_shortest_or_rounded_with_ties_up() {
        assert_eq!(digits(0.1 + 0.2, None), Some(("30000000000000004".into(), -1)));
        assert_eq!(digits(1e21, None), Some(("1".into(), 21)));
        assert_eq!(digits(2.5, Some(1)), Some(("3".into(), 0)));
        assert_eq!(digits(99.5, Some(2)), Some(("10".into(), 2)));
        assert_eq!(digits(0.1, Some(20)), Some(("10000000000000000555".into(), -1)));
        assert_eq!(digits(5e-324, Some(1)), Some(("5".into(), -324)));
        assert_eq!(digits(0.0, None), None);
    }

    #[test]
    fn whole_numbers_convert_exactly_across_radixes() {
        assert_eq!(big_integer("20000000000001", 16).as_deref(), Some("9007199254740993"));
        assert_eq!(big_integer("12", 2), None);
        assert_eq!(parse_number("ff", 16), Some(255.0));
        assert_eq!(parse_number("-.5e1", 10), Some(-5.0));
        assert_eq!(parse_number("inf", 10), None);
        assert_eq!(integer_radix(2f64.powi(128), 16), Some(format!("1{}", "0".repeat(32))));
        assert_eq!(integer_radix(f64::MAX, 2), Some(format!("{}{}", "1".repeat(53), "0".repeat(971))));
        assert_eq!(integer_radix(1.5, 10), None);
        assert_eq!(compare_integers("-9007199254740993", "-9007199254740992"), Some(Ordering::Less));
        assert_eq!(compare_integers("-0", "00"), Some(Ordering::Equal));
        assert_eq!(compare_integers("1.5", "1"), None);
    }

    #[test]
    fn int32_wraps_like_modular_arithmetic() {
        assert_eq!(int32(4_294_967_296.0 + 5.0), 5);
        assert_eq!(int32(2_147_483_648.0), i32::MIN);
        assert_eq!(int32(-1.5), -1);
        assert_eq!(int32(f64::NAN), 0);
    }
}
