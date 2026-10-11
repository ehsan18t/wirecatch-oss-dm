//! Constants the TypeScript apps need, generated next to the types.
//!
//! `ts-rs` generates types but not values. A constant declared with [`exported_consts!`] is an
//! ordinary Rust constant, and the generator test in this module also writes it, with its
//! name, value and doc comment, into `constants.ts` in the folder `ts-rs` writes to, so the
//! apps import it from `@wirecatch/protocol` instead of repeating the number. Tables that
//! belong to a type, such as the audio bit rates of each [`OutputFormat`], are written from
//! that type's methods.
//!
//! [`OutputFormat`]: crate::app::OutputFormat

/// Declares public constants that are also written into the generated TypeScript.
///
/// Each constant needs a doc comment; it becomes the TypeScript doc comment too. A module
/// using this macro gets a test-only `exported_consts()` function, which the generator in
/// [`crate::exported`] lists.
macro_rules! exported_consts {
    ($( $(#[doc = $doc:literal])+ pub const $name:ident: $ty:ty = $value:expr; )+) => {
        $(
            $(#[doc = $doc])+
            pub const $name: $ty = $value;
        )+

        /// This module's constants for the TypeScript generator.
        #[cfg(test)]
        pub(crate) fn exported_consts() -> Vec<crate::exported::Constant> {
            vec![$(crate::exported::Constant::number(stringify!($name), &[$($doc),+], $name)),+]
        }
    };
}

pub(crate) use exported_consts;

/// One constant as it is written into TypeScript.
#[cfg(test)]
pub(crate) struct Constant {
    name: &'static str,
    docs: Vec<&'static str>,
    /// The TypeScript type annotation, when the value's inferred type is not the one wanted.
    annotation: Option<&'static str>,
    value: String,
}

#[cfg(test)]
impl Constant {
    /// A whole number. TypeScript numbers are exact only up to 2^53, so a larger one fails
    /// the generator instead of being written wrong.
    pub(crate) fn number(
        name: &'static str,
        docs: &[&'static str],
        value: impl TryInto<u64>,
    ) -> Constant {
        /// `Number.MAX_SAFE_INTEGER` in JavaScript.
        const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
        let value: u64 = value
            .try_into()
            .ok()
            .filter(|v| *v <= MAX_SAFE_INTEGER)
            .unwrap_or_else(|| panic!("{name} is not exact as a TypeScript number"));
        Constant {
            name,
            docs: docs.to_vec(),
            annotation: None,
            value: value.to_string(),
        }
    }

    fn write(&self, out: &mut String) {
        use std::fmt::Write as _;
        out.push_str("\n/**\n");
        for line in &self.docs {
            // `///` comments arrive with the space after the slashes.
            let line = line.strip_prefix(' ').unwrap_or(line);
            if line.is_empty() {
                out.push_str(" *\n");
            } else {
                writeln!(out, " * {line}").unwrap();
            }
        }
        out.push_str(" */\n");
        let annotation = self
            .annotation
            .map(|a| format!(": {a}"))
            .unwrap_or_default();
        writeln!(
            out,
            "export const {}{annotation} = {};",
            self.name, self.value
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Value;

    use super::Constant;
    use crate::app::OutputFormat;

    /// The file written into the generated folder. `packages/protocol/scripts/write-index.mjs`
    /// re-exports it from the package by this name.
    const FILE_NAME: &str = "constants.ts";

    /// Every output format, in the order the tables list them. Each format names the one
    /// after it, so a new format does not compile until it is given a place in the chain.
    fn output_formats() -> Vec<OutputFormat> {
        let mut all = Vec::new();
        let mut next = Some(OutputFormat::Original);
        while let Some(format) = next {
            all.push(format);
            next = match format {
                OutputFormat::Original => Some(OutputFormat::Mp4),
                OutputFormat::Mp4 => Some(OutputFormat::Mkv),
                OutputFormat::Mkv => Some(OutputFormat::Webm),
                OutputFormat::Webm => Some(OutputFormat::Mp3),
                OutputFormat::Mp3 => Some(OutputFormat::M4a),
                OutputFormat::M4a => Some(OutputFormat::Opus),
                OutputFormat::Opus => Some(OutputFormat::Flac),
                OutputFormat::Flac => Some(OutputFormat::Wav),
                OutputFormat::Wav => None,
            };
        }
        all
    }

    /// A TypeScript object with one entry per format that has a value, keyed by the format's
    /// name on the wire.
    fn per_format(
        name: &'static str,
        docs: &[&'static str],
        annotation: &'static str,
        value_of: impl Fn(OutputFormat) -> Option<String>,
    ) -> Constant {
        let entries: Vec<String> = output_formats()
            .into_iter()
            .filter_map(|format| {
                let Value::String(key) = serde_json::to_value(format).unwrap() else {
                    panic!("{format:?} is not written as a string");
                };
                value_of(format).map(|value| format!("  {key}: {value},"))
            })
            .collect();
        Constant {
            name,
            docs: docs.to_vec(),
            annotation: Some(annotation),
            value: format!("{{\n{}\n}}", entries.join("\n")),
        }
    }

    fn audio_tables() -> Vec<Constant> {
        let list = |values: &[u32]| {
            let values: Vec<String> = values.iter().map(u32::to_string).collect();
            format!("[{}]", values.join(", "))
        };
        vec![
            per_format(
                "AUDIO_KBPS_CHOICES",
                &[
                    " The audio bit rates, in kbps, a person can pick when saving as each format;",
                    " formats without a choice are left out (`OutputFormat::kbps_choices`).",
                ],
                "Readonly<Partial<Record<OutputFormat, readonly number[]>>>",
                |format| {
                    Some(format.kbps_choices())
                        .filter(|c| !c.is_empty())
                        .map(list)
                },
            ),
            per_format(
                "AUDIO_DEFAULT_KBPS",
                &[
                    " The audio bit rate, in kbps, used when the audio of a download saved as each",
                    " format must be re-encoded (`OutputFormat::default_kbps`).",
                ],
                "Readonly<Partial<Record<OutputFormat, number>>>",
                |format| format.default_kbps().map(|kbps| kbps.to_string()),
            ),
        ]
    }

    /// Where `ts-rs` writes the types: `TS_RS_EXPORT_DIR` (set in `.cargo/config.toml`), or
    /// its own default.
    fn export_dir() -> PathBuf {
        std::env::var_os("TS_RS_EXPORT_DIR")
            .map_or_else(|| PathBuf::from("./bindings"), PathBuf::from)
    }

    #[test]
    fn write_typescript_constants() {
        let constants = [
            crate::exported_consts(),
            crate::wire::exported_consts(),
            crate::frame::exported_consts(),
            crate::app::exported_consts(),
            audio_tables(),
        ];
        let mut out = String::from(
            "// Generated by `cargo test -p wirecatch-protocol` from crates/protocol. Do not edit; run `pnpm protocol:generate` after changing the constants there.\n\nimport type { OutputFormat } from \"./OutputFormat\";\n",
        );
        for constant in constants.iter().flatten() {
            constant.write(&mut out);
        }
        let dir = export_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), out).unwrap();
    }
}
