//! Timezone whitelist and offset validation for the F20 revision prompt.
//!
//! Vespertide deliberately avoids pulling in `chrono-tz` (which would add a
//! multi-megabyte `IANA` database to the CLI binary). Instead the user picks
//! from a curated 30-name whitelist covering global hot spots, or types a
//! numeric `±HH:MM` offset. The selected string is stored verbatim on
//! `MigrationAction::ModifyColumnType.timezone` and forwarded into the
//! `PostgreSQL` `AT TIME ZONE '<tz>'` clause at SQL-generation time —
//! `PostgreSQL` accepts both `IANA` names and numeric offsets.
//!
//! The whitelist is intentionally global: at least one entry from every
//! inhabited continent so users on most teams find a recognisable option
//! without falling back to the custom input.

/// IANA timezone names accepted by the Select UI without further validation.
/// Stays in ASCII sort order so the Select list is deterministic.
pub(super) const KNOWN_IANA: &[&str] = &[
    // UTC pinned to the first slot as the safe default. The remaining 29
    // entries are in strict ASCII sort order so the Select UI is bisectable
    // and `cargo insta` snapshots are deterministic.
    "UTC",
    "Africa/Cairo",
    "Africa/Johannesburg",
    "Africa/Lagos",
    "America/Buenos_Aires",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Mexico_City",
    "America/New_York",
    "America/Sao_Paulo",
    "America/Toronto",
    "Asia/Bangkok",
    "Asia/Dubai",
    "Asia/Hong_Kong",
    "Asia/Kolkata",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Tokyo",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Europe/Berlin",
    "Europe/London",
    "Europe/Madrid",
    "Europe/Moscow",
    "Europe/Paris",
    "Europe/Rome",
    "Pacific/Auckland",
];

/// Result of validating user input against the IANA whitelist and the
/// numeric `±HH:MM` offset format.
pub(super) fn validate_timezone(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Timezone is required.".to_string());
    }
    if KNOWN_IANA.contains(&trimmed) {
        return Ok(trimmed.to_string());
    }
    validate_offset(trimmed)
        .map(|()| trimmed.to_string())
        .map_err(|why| {
            format!(
                "'{trimmed}' is not in the IANA whitelist and is not a valid numeric offset \
             ({why}). Use one of {:?} or a literal like '+09:00'.",
                KNOWN_IANA.iter().take(5).collect::<Vec<_>>()
            )
        })
}

/// Validate numeric UTC offset in the SQL-portable `±HH:MM` format.
/// Bounds match IANA's actual range: hours `[00, 14]`, minutes `[00, 59]`.
/// Returns `Err` with a human-readable reason on rejection.
pub(super) fn validate_offset(input: &str) -> Result<(), String> {
    let bytes = input.as_bytes();
    if bytes.len() != 6 {
        return Err(format!("expected 6 characters, got {}", bytes.len()));
    }
    if bytes[0] != b'+' && bytes[0] != b'-' {
        return Err("must start with '+' or '-'".to_string());
    }
    if bytes[3] != b':' {
        return Err("hours and minutes must be separated by ':'".to_string());
    }
    let hh =
        parse_two_digit(&bytes[1..3]).ok_or_else(|| "hour part must be two digits".to_string())?;
    let mm = parse_two_digit(&bytes[4..6])
        .ok_or_else(|| "minute part must be two digits".to_string())?;
    if hh > 14 {
        return Err(format!("hour {hh} exceeds maximum (14)"));
    }
    if mm > 59 {
        return Err(format!("minute {mm} exceeds 59"));
    }
    // The actual extreme +14:00 / -12:00 boundary is enforced by hh range; we
    // intentionally allow -14:00 too because some historical zones reached it.
    Ok(())
}

fn parse_two_digit(bytes: &[u8]) -> Option<u8> {
    if bytes.len() != 2 || !bytes[0].is_ascii_digit() || !bytes[1].is_ascii_digit() {
        return None;
    }
    Some((bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_is_global_and_in_sort_order() {
        // Sanity: every region represented.
        assert!(KNOWN_IANA.contains(&"UTC"));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("Africa/")));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("America/")));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("Asia/")));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("Europe/")));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("Australia/")));
        assert!(KNOWN_IANA.iter().any(|n| n.starts_with("Pacific/")));

        // UTC is intentionally pinned to the first slot so the Select UI
        // surfaces the safe default at the top regardless of ASCII order.
        assert_eq!(KNOWN_IANA[0], "UTC", "UTC must lead the list as default");

        // The remaining 29 entries stay in ASCII sort order so the Select
        // UI is deterministic and bisectable.
        let tail: Vec<&str> = KNOWN_IANA.iter().skip(1).copied().collect();
        let mut sorted_tail = tail.clone();
        sorted_tail.sort_unstable();
        assert_eq!(sorted_tail, tail, "non-UTC entries must stay sorted");
    }

    #[test]
    fn whitelist_size_is_exactly_thirty() {
        // Locked at 30 per design discussion; raising it requires updating
        // the prompt UI footer that mentions the size.
        assert_eq!(
            KNOWN_IANA.len(),
            30,
            "whitelist must contain exactly 30 entries"
        );
    }

    #[test]
    fn whitelist_entries_round_trip_validate() {
        for name in KNOWN_IANA {
            assert_eq!(validate_timezone(name).as_deref(), Ok(*name));
        }
    }

    #[test]
    fn valid_offsets_accept() {
        for ok in &[
            "+00:00", "-00:00", "+09:00", "-05:00", "+05:30", "+14:00", "-12:00",
        ] {
            assert!(validate_offset(ok).is_ok(), "{ok} should validate");
            assert_eq!(validate_timezone(ok).as_deref(), Ok(*ok));
        }
    }

    #[test]
    fn invalid_offsets_reject_with_reason() {
        let cases = [
            ("", "6 characters"),
            ("+9:00", "6 characters"),
            ("09:00:00", "6 characters"),
            ("*09:00", "'+' or '-'"),
            ("+09-00", "':'"),
            ("+aa:00", "two digits"),
            ("+09:ab", "two digits"),
            ("+15:00", "exceeds maximum"),
            ("+09:60", "exceeds 59"),
        ];
        for (input, expected_fragment) in cases {
            let err = validate_offset(input)
                .err()
                .unwrap_or_else(|| panic!("'{input}' should fail validation"));
            assert!(
                err.contains(expected_fragment),
                "error for '{input}' should mention '{expected_fragment}': {err}"
            );
        }
    }

    // mm == 59 is the LAST valid minute. Pins `mm > 59` (a `>=` mutant would
    // reject the legal `:59` boundary).
    #[test]
    fn offset_minute_fifty_nine_is_valid() {
        assert!(validate_offset("+12:59").is_ok());
    }

    // parse_two_digit's reject guard is `len != 2 || !digit0 || !digit1`. These
    // pin each `||`: a `&&` mutant would parse a non-two-digit / non-digit slice
    // (and, on a 1-byte slice, index out of bounds).
    #[test]
    fn parse_two_digit_rejects_non_digit_in_each_position() {
        assert_eq!(parse_two_digit(b"55"), Some(55));
        assert_eq!(parse_two_digit(b"x5"), None, "non-digit first position");
        assert_eq!(parse_two_digit(b"5x"), None, "non-digit second position");
        assert_eq!(
            parse_two_digit(b"5"),
            None,
            "short slice must not be parsed"
        );
    }

    #[test]
    fn empty_input_rejects_with_explicit_message() {
        let err = validate_timezone("").unwrap_err();
        assert!(err.contains("required"), "got: {err}");
    }

    #[test]
    fn unknown_iana_name_rejects_with_suggestion() {
        let err = validate_timezone("Asia/Sakhalin").unwrap_err();
        assert!(err.contains("Asia/Sakhalin"), "should echo input: {err}");
        assert!(
            err.contains("'+09:00'"),
            "should suggest offset syntax: {err}"
        );
    }

    #[test]
    fn input_with_surrounding_whitespace_is_trimmed() {
        assert_eq!(validate_timezone("  UTC  ").as_deref(), Ok("UTC"));
        assert_eq!(validate_timezone("\t+09:00\n").as_deref(), Ok("+09:00"));
    }
}
