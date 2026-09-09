pub fn is_sensitive_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["token", "secret", "password", "key", "env", "credential"]
        .iter()
        .any(|frag| lower.contains(frag))
}

/// Converts a Unix timestamp (whole seconds since the epoch, UTC — never
/// local time; see `config::DateSource`/`render::format_timestamp`, which
/// both display everything in UTC for the same reason: the standard
/// library exposes no portable way to read the local timezone, and a
/// fixed, explicit UTC convention keeps every date Sift shows or acts on
/// fully deterministic and machine-independent) into proleptic Gregorian
/// calendar components `(year, month, day)`. Implements the well-known
/// `civil_from_days` algorithm (Howard Hinnant), exact over the full
/// `i64` range for non-negative input. The single such implementation in
/// the crate — both `render::format_timestamp` and the Date organize
/// strategy call this rather than each computing it themselves.
pub fn civil_from_unix_secs(secs: i64) -> (i64, u32, u32) {
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year, month as u32, day as u32)
}
