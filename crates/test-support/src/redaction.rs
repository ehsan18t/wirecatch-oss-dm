//! The samples the redaction tests read, from `data/redaction.json`: the one file the log's tests (`wirecatch-logging`) and the extension's (`apps/extension`, through `@wirecatch/protocol/testing/redaction`) both load, so the two redactions are held to the same cases (D40.10).
//!
//! A text, secret or rule input in the file may be a list of pieces, joined here, so no file in the repository holds a string a secret scanner takes for a real credential.

use std::sync::OnceLock;

use serde::Deserialize;

/// The file, as the crate is built with it.
const DATA: &str = include_str!("../data/redaction.json");

/// What `{}` stands for in a wrapping.
const PLACE: &str = "{}";

/// One of the ways text is redacted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// `wirecatch_logging::redact::text_for_log`.
    Log,
    /// `wirecatch_logging::redact::text_for_display`, which hides only the passwords of links.
    Display,
    /// The extension's `redact`.
    Diagnostics,
}

/// A text, written whole or as pieces.
#[derive(Deserialize)]
#[serde(untagged)]
enum Pieces {
    Whole(String),
    Parts(Vec<String>),
}

impl Pieces {
    fn joined(self) -> String {
        match self {
            Self::Whole(text) => text,
            Self::Parts(parts) => parts.concat(),
        }
    }
}

fn joined<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Pieces::deserialize(deserializer).map(Pieces::joined)
}

fn all_joined<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    Vec::<Pieces>::deserialize(deserializer)
        .map(|all| all.into_iter().map(Pieces::joined).collect())
}

/// One sample and what each side makes of it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionCase {
    /// The text, its pieces joined.
    #[serde(deserialize_with = "joined")]
    pub text: String,
    /// What neither the log nor the diagnostics may keep, in any of the [`wrappings`] (unless one skips the case).
    #[serde(default, deserialize_with = "all_joined")]
    pub secrets: Vec<String>,
    /// What the diagnostics may not keep (an address's path, a person's name), which the log keeps on purpose.
    #[serde(default, deserialize_with = "all_joined")]
    pub private: Vec<String>,
    /// What `text_for_log` makes of the text.
    #[serde(default)]
    pub log: Option<String>,
    /// What `text_for_display` makes of the text.
    #[serde(default)]
    pub display: Option<String>,
    /// What the extension's `redact` makes of the text.
    #[serde(default)]
    pub diagnostics: Option<String>,
    /// The sides that leave the text as it is.
    #[serde(default)]
    pub kept_by: Vec<Side>,
    /// The redaction that does not read this form on purpose.
    #[serde(default)]
    pub skipped_by: Option<Side>,
    /// Why it does not.
    #[serde(default)]
    pub why: Option<String>,
}

impl RedactionCase {
    /// What `side` must make of the text, if the case says.
    #[must_use]
    pub fn expected(&self, side: Side) -> Option<&str> {
        let exact = match side {
            Side::Log => &self.log,
            Side::Display => &self.display,
            Side::Diagnostics => &self.diagnostics,
        };
        exact
            .as_deref()
            .or_else(|| self.kept_by.contains(&side).then_some(self.text.as_str()))
    }

    /// The secrets `side` must hide: none when it skips the case, or for [`Side::Display`].
    #[must_use]
    pub fn secrets_for(&self, side: Side) -> &[String] {
        if side == Side::Display || self.skipped_by == Some(side) {
            &[]
        } else {
            &self.secrets
        }
    }
}

/// One input of a rule, and its answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleCase {
    /// The input, its pieces joined.
    #[serde(deserialize_with = "joined")]
    pub input: String,
    /// What the rule answers.
    pub answer: bool,
}

#[derive(Deserialize)]
struct Data {
    #[serde(rename = "about")]
    _about: Vec<String>,
    wrappings: Vec<String>,
    rules: std::collections::BTreeMap<String, Vec<RuleCase>>,
    cases: Vec<RedactionCase>,
}

fn data() -> &'static Data {
    static DATA_READ: OnceLock<Data> = OnceLock::new();
    DATA_READ.get_or_init(|| {
        serde_json::from_str(DATA).unwrap_or_else(|e| panic!("data/redaction.json: {e}"))
    })
}

/// Every sample.
#[must_use]
pub fn redaction_cases() -> &'static [RedactionCase] {
    &data().cases
}

/// The cases of the rule `name` (`is_secret_name`, `names_a_password`, `looks_like_a_credential`, `is_service_key`).
///
/// # Panics
///
/// When the file holds no cases for `name`.
#[must_use]
pub fn rule_cases(name: &str) -> &'static [RuleCase] {
    data()
        .rules
        .get(name)
        .unwrap_or_else(|| panic!("data/redaction.json holds no cases of {name}"))
}

/// `text` in each of the surroundings a secret is tested in: alone, in a sentence, in brackets and between lines.
#[must_use]
pub fn wrappings(text: &str) -> Vec<String> {
    data()
        .wrappings
        .iter()
        .map(|wrapping| wrapping.replace(PLACE, text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_joined_wherever_they_are_written() {
        let case: RedactionCase =
            serde_json::from_str(r#"{"text": ["a", "b", "c"], "secrets": ["x", ["y", "z"]]}"#)
                .unwrap();
        assert_eq!(case.text, "abc");
        assert_eq!(case.secrets, ["x", "yz"]);
        let rule: RuleCase =
            serde_json::from_str(r#"{"input": ["k", "ey"], "answer": true}"#).unwrap();
        assert_eq!(rule.input, "key");
    }

    #[test]
    fn every_case_says_what_it_tests_and_holds_what_it_names() {
        assert_ne!(
            redaction_cases().len(),
            0,
            "data/redaction.json holds no case"
        );
        for case in redaction_cases() {
            let text = &case.text;
            assert!(
                !case.secrets.is_empty()
                    || !case.private.is_empty()
                    || !case.kept_by.is_empty()
                    || case.log.is_some()
                    || case.display.is_some()
                    || case.diagnostics.is_some(),
                "{text:?} tests nothing"
            );
            for secret in case.secrets.iter().chain(&case.private) {
                assert!(
                    text.contains(secret.as_str()),
                    "{text:?} does not hold {secret:?}"
                );
            }
            assert!(
                case.kept_by.is_empty() || case.secrets.is_empty(),
                "{text:?} is kept and has secrets"
            );
            assert_eq!(
                case.skipped_by.is_some(),
                case.why.is_some(),
                "{text:?}: a skip and its reason go together"
            );
            if let Some(side) = case.skipped_by {
                assert!(
                    case.expected(side).is_none(),
                    "{text:?} is skipped by {side:?} and expected of it"
                );
            }
        }
    }

    #[test]
    fn each_rule_has_cases_and_each_wrapping_places_the_text() {
        for rule in [
            "is_secret_name",
            "names_a_password",
            "looks_like_a_credential",
            "is_service_key",
        ] {
            assert_ne!(rule_cases(rule).len(), 0, "{rule}");
        }
        let wrapped = wrappings("TEXT");
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|w| w.contains("TEXT")), "{wrapped:?}");
    }
}
