//! The samples are in `crates/test-support/data/redaction.json`, which the extension's tests load too (D40.10); only what tests this crate's own code stays here.

use wirecatch_test_support::redaction::{Side, redaction_cases, wrappings};

use super::*;

/// Every sample with an expected log line or link for a person gives it exactly.
#[test]
fn each_shared_case_reads_as_expected() {
    let mut wrong = Vec::new();
    for case in redaction_cases() {
        for (side, redact) in [
            (Side::Log, text_for_log as fn(&str) -> String),
            (Side::Display, text_for_display),
        ] {
            if let Some(expected) = case.expected(side) {
                let got = redact(&case.text);
                if got != expected {
                    wrong.push(format!(
                        "{side:?}: {:?} gave {got:?}, not {expected:?}",
                        case.text
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// No secret of a shared sample survives the log, in any of the shared wrappings.
#[test]
fn no_shared_secret_survives_the_log() {
    let mut leaked = Vec::new();
    for case in redaction_cases() {
        let secrets = case.secrets_for(Side::Log);
        for wrapped in wrappings(&case.text) {
            let logged = text_for_log(&wrapped);
            if secrets
                .iter()
                .any(|secret| logged.contains(secret.as_str()))
            {
                leaked.push(format!("{wrapped:?} was logged as {logged:?}"));
            }
        }
    }
    assert!(leaked.is_empty(), "{}", leaked.join("\n"));
}

#[test]
fn a_link_for_a_person_is_the_text_for_a_person() {
    for case in redaction_cases() {
        if let Some(expected) = case.expected(Side::Display) {
            assert_eq!(url_for_display(&case.text), expected);
        }
    }
    assert_eq!(optional_url_for_display(None), None);
}

#[test]
fn a_long_hostile_line_is_redacted_in_linear_time() {
    let size = 256 * 1024;
    for unit in [
        "a_token=1 ",
        "a://",
        "u:x://",
        "password=\"",
        "Bearer ",
        "eyJ.",
        "?a=1&",
        "\\\"token\\\":",
        "cookie: ",
        "--password ",
        "\"value\":\"a\",",
        "name: a, value: ",
        "token: [{\"",
        "Cookie: a\n b",
        "Authorization: x\nb",
        "a=Bearer ",
        "token: Basic\n",
        "x=y=z=",
    ] {
        let line = unit.repeat(size / unit.len());
        steps::reset();
        let logged = text_for_log(&line);
        let taken = steps::taken();
        assert!(
            taken <= 16 * line.len(),
            "{unit:?}: {taken} steps for {} bytes",
            line.len()
        );
        assert!(
            logged.len() <= 2 * line.len(),
            "{unit:?}: the line grew to {} bytes",
            logged.len()
        );
    }
}
