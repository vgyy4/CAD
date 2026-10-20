//! `std::ostream` formatting of numbers (`printf("%.{p}g")` and `printf("%{w}.{p}f")` style output).

fn strip_zeros(s: &str) -> &str {
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { s }
}

/// A `double` written to a `std::ostream` with the default float format and the stream precision `precision`
/// (`%.{precision}g`; a precision of 0 acts as 1).
pub(crate) fn ostream_g(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan".to_string() } else { "nan".to_string() };
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".to_string() } else { "inf".to_string() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    let p = precision.max(1);
    let sci = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = match sci.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (sci.as_str(), 0),
    };
    if exp < -4 || i64::from(exp) >= p as i64 {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", strip_zeros(mantissa), sign, exp.abs())
    } else {
        let decimals = usize::try_from(p as i64 - 1 - i64::from(exp)).unwrap_or(0);
        let fixed = format!("{:.*}", decimals, v);
        strip_zeros(&fixed).to_string()
    }
}

/// `std::setw(width)` applied to `text`: right aligned.
pub(crate) fn setw(text: &str, width: usize) -> String {
    format!("{text:>width$}")
}

/// `printf("%{width}.{decimals}f")`.
pub(crate) fn fixed(v: f64, width: usize, decimals: usize) -> String {
    if v.is_nan() {
        return setw(if v.is_sign_negative() { "-nan" } else { "nan" }, width);
    }
    if v.is_infinite() {
        return setw(if v < 0.0 { "-inf" } else { "inf" }, width);
    }
    setw(&format!("{v:.decimals$}"), width)
}
