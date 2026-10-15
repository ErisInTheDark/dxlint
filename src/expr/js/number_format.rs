//! JavaScript number formatting beyond `String(n)`: `toPrecision` and
//! whole-number radix text, exact over the full double range.

/// `Number.prototype.toPrecision(digits)`; `None` outside 1..=100 digits.
pub fn to_precision(value: f64, digits: usize) -> Option<String> {
    if !(1..=100).contains(&digits) { return None; }
    if value.is_nan() { return Some("NaN".into()); }
    if value.is_infinite() { return Some(if value > 0.0 { "Infinity" } else { "-Infinity" }.into()); }
    let sign = if value < 0.0 { "-" } else { "" };
    let (mantissa, exponent) = if value == 0.0 { ("0".repeat(digits), 0) } else { rounded(value.abs(), digits) };
    let text = if exponent < -6 || exponent >= digits as i32 {
        let (head, tail) = mantissa.split_at(1);
        let fraction = if tail.is_empty() { String::new() } else { format!(".{tail}") };
        format!("{head}{fraction}e{}{}", if exponent < 0 { '-' } else { '+' }, exponent.abs())
    } else if exponent >= 0 {
        let whole = exponent as usize + 1;
        if whole == digits { mantissa } else { format!("{}.{}", &mantissa[..whole], &mantissa[whole..]) }
    } else {
        format!("0.{}{mantissa}", "0".repeat((-exponent - 1) as usize))
    };
    Some(format!("{sign}{text}"))
}

/// `digits` significant digits of a positive finite `value` and the decimal
/// exponent of the first, rounding exact ties up as JavaScript does.
fn rounded(value: f64, digits: usize) -> (String, i32) {
    // A double's exact decimal expansion has at most 767 significant digits.
    let exact = format!("{value:.800e}");
    let (coefficient, exponent) = exact.split_once('e').expect("exponent form");
    let mut exponent: i32 = exponent.parse().expect("decimal exponent");
    let all: Vec<u8> = coefficient.bytes().filter(u8::is_ascii_digit).collect();
    let mut kept = all[..digits].to_vec();
    if all[digits] >= b'5' {
        let mut at = digits;
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
    (String::from_utf8(kept).expect("ascii digits"), exponent)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_precision_matches_javascript() {
        for (value, digits, expected) in [
            (1.0, 3, "1.00"), (2.5, 1, "3"), (0.0000001, 2, "1.0e-7"), (9.99, 2, "10"),
            (123456.0, 2, "1.2e+5"), (0.000123, 2, "0.00012"), (-1.5, 1, "-2"), (0.0, 3, "0.00"),
            (-0.0, 1, "0"), (1e21, 3, "1.00e+21"), (5e-324, 1, "5e-324"), (0.1, 20, "0.10000000000000000555"),
            (99.5, 2, "1.0e+2"), (f64::INFINITY, 2, "Infinity"),
        ] {
            assert_eq!(to_precision(value, digits).as_deref(), Some(expected), "{value} {digits}");
        }
        assert_eq!(to_precision(1.0, 0), None);
        assert_eq!(to_precision(1.0, 101), None);
    }

    #[test]
    fn integer_radix_covers_whole_doubles() {
        assert_eq!(integer_radix(255.0, 16).as_deref(), Some("ff"));
        assert_eq!(integer_radix(16.0, 2).as_deref(), Some("10000"));
        assert_eq!(integer_radix(-35.0, 36).as_deref(), Some("-z"));
        assert_eq!(integer_radix(2f64.powi(128), 16), Some(format!("1{}", "0".repeat(32))));
        assert_eq!(integer_radix(f64::MAX, 2), Some(format!("{}{}", "1".repeat(53), "0".repeat(971))));
        assert_eq!(integer_radix(1.5, 10), None);
        assert_eq!(integer_radix(10.0, 37), None);
    }
}
