//! `std::ostream` formatting of a `double` (`printf("%g")`, six significant digits).

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

/// Text of a `double` written to a default `std::ostream`.
pub(crate) fn ostream_double(v: f64) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".to_string() } else { "inf".to_string() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    let sci = format!("{:.5e}", v);
    let (mantissa, exp) = match sci.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (sci.as_str(), 0),
    };
    if !(-4..6).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip_zeros(mantissa), sign, exp.abs())
    } else {
        let decimals = usize::try_from(5 - exp).unwrap_or(0);
        let fixed = format!("{:.*}", decimals, v);
        strip_zeros(&fixed).to_string()
    }
}
