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
/// Each constant needs a doc comment; it becomes the TypeScript doc comment too. Its type must
/// implement [`Exported`], which says how it is written. A module using this macro gets a
/// test-only `exported_consts()` function, which the generator in [`crate::exported`] lists.
macro_rules! exported_consts {
    ($( $(#[doc = $doc:literal])+ pub const $name:ident: $ty:ty = $value:expr; )+) => {
        $(
            $(#[doc = $doc])+
            pub const $name: $ty = $value;
        )+

        /// This module's constants for the TypeScript generator.
        #[cfg(test)]
        pub(crate) fn exported_consts() -> Vec<crate::exported::Constant> {
            vec![$(crate::exported::Constant::of(stringify!($name), &[$($doc),+], &$name)),+]
        }
    };
}

pub(crate) use exported_consts;

/// A value as it is written into TypeScript.
#[cfg(test)]
pub(crate) struct Written {
    /// The TypeScript expression.
    value: String,
    /// The TypeScript type annotation, when the value's inferred type is not the one wanted.
    annotation: Option<&'static str>,
    /// Generated types the annotation names, imported into the file.
    imports: &'static [&'static str],
}

/// A Rust value the generator can write as a TypeScript constant.
#[cfg(test)]
pub(crate) trait Exported {
    /// The value written as TypeScript.
    fn written(&self) -> Written;
}

/// Whole numbers. TypeScript numbers are exact only between -(2^53 - 1) and 2^53 - 1, so a
/// value outside that range fails the generator instead of being written wrong.
#[cfg(test)]
fn whole_number(value: impl TryInto<i128>) -> Written {
    /// `Number.MAX_SAFE_INTEGER` in JavaScript.
    const MAX_SAFE_INTEGER: i128 = (1 << 53) - 1;
    let value: i128 = value
        .try_into()
        .ok()
        .filter(|v| v.abs() <= MAX_SAFE_INTEGER)
        .unwrap_or_else(|| {
            panic!("a constant is outside the range a TypeScript number holds exactly (2^53 - 1 either way)")
        });
    Written {
        value: value.to_string(),
        annotation: None,
        imports: &[],
    }
}

macro_rules! exported_numbers {
    ($($ty:ty),+) => {
        $(
            #[cfg(test)]
            impl Exported for $ty {
                fn written(&self) -> Written {
                    whole_number(*self)
                }
            }
        )+
    };
}

exported_numbers!(u8, u16, u32, u64, usize, i64);

#[cfg(test)]
impl Exported for &str {
    fn written(&self) -> Written {
        Written {
            value: serde_json::to_string(self).unwrap(),
            annotation: None,
            imports: &[],
        }
    }
}

#[cfg(test)]
impl Exported for &[&str] {
    fn written(&self) -> Written {
        Written {
            value: serde_json::to_string(self).unwrap(),
            annotation: Some("readonly string[]"),
            imports: &[],
        }
    }
}

#[cfg(test)]
impl Exported for crate::QueueId {
    fn written(&self) -> Written {
        Written {
            annotation: Some("QueueId"),
            imports: &["QueueId"],
            ..whole_number(self.0)
        }
    }
}

/// One constant as it is written into TypeScript.
#[cfg(test)]
pub(crate) struct Constant {
    name: &'static str,
    docs: Vec<&'static str>,
    written: Written,
}

#[cfg(test)]
impl Constant {
    /// The constant `name`, documented by `docs`, holding `value`.
    pub(crate) fn of(name: &'static str, docs: &[&'static str], value: &impl Exported) -> Constant {
        Constant {
            name,
            docs: docs.to_vec(),
            written: value.written(),
        }
    }

    /// A whole number.
    pub(crate) fn number(
        name: &'static str,
        docs: &[&'static str],
        value: impl TryInto<i128>,
    ) -> Constant {
        Constant {
            name,
            docs: docs.to_vec(),
            written: whole_number(value),
        }
    }

    /// A value written as its JSON, of the generated type `annotation`, for a default the apps
    /// start from before the app tells them better.
    pub(crate) fn json(
        name: &'static str,
        docs: &[&'static str],
        annotation: &'static str,
        imports: &'static [&'static str],
        value: &impl serde::Serialize,
    ) -> Constant {
        Constant {
            name,
            docs: docs.to_vec(),
            written: Written {
                value: serde_json::to_string_pretty(value).unwrap(),
                annotation: Some(annotation),
                imports,
            },
        }
    }

    fn write(&self, out: &mut String) {
        use std::fmt::Write as _;
        out.push_str("\n/**\n");
        for line in self.docs.iter().flat_map(|doc| doc_lines(doc)) {
            // `*/` inside the text would end the TypeScript comment early.
            let line = line.replace("*/", "*\\/");
            if line.is_empty() {
                out.push_str(" *\n");
            } else {
                writeln!(out, " * {line}").unwrap();
            }
        }
        out.push_str(" */\n");
        let annotation = self
            .written
            .annotation
            .map(|a| format!(": {a}"))
            .unwrap_or_default();
        writeln!(
            out,
            "export const {}{annotation} = {};",
            self.name, self.written.value
        )
        .unwrap();
    }
}

/// The lines of one doc attribute. A `///` comment is one attribute per line, with the
/// space after the slashes; a `/** */` comment is one attribute holding every line, each
/// usually starting with ` * `, and the line breaks after `/**` and before `*/`.
#[cfg(test)]
fn doc_lines(doc: &str) -> Vec<&str> {
    if !doc.contains('\n') {
        return vec![doc.strip_prefix(' ').unwrap_or(doc)];
    }
    let lines: Vec<&str> = doc
        .lines()
        .map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix('*').unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line).trim_end()
        })
        .collect();
    let first = lines
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(first, |l| l + 1);
    lines[first..last].to_vec()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Value;

    use super::Constant;
    use crate::app::OutputFormat;
    use crate::extension::ExtensionSettings;

    /// The file written into the generated folder. `packages/protocol/scripts/index-source.mjs`
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
            written: super::Written {
                value: format!("{{\n{}\n}}", entries.join("\n")),
                annotation: Some(annotation),
                imports: &["OutputFormat"],
            },
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

    /// Where `ts-rs` writes the types: `TS_RS_EXPORT_DIR`, which `.cargo/config.toml` points into `target/` for plain test runs and `pnpm protocol:generate` points at a staging folder, or its own default.
    fn export_dir() -> PathBuf {
        std::env::var_os("TS_RS_EXPORT_DIR")
            .map_or_else(|| PathBuf::from("./bindings"), PathBuf::from)
    }

    fn written(constant: &Constant) -> String {
        let mut out = String::new();
        constant.write(&mut out);
        out
    }

    #[test]
    fn line_comments_become_one_comment_line_each() {
        let out = written(&Constant::number("A", &[" First.", "", " Second."], 1u8));
        assert_eq!(
            out,
            "\n/**\n * First.\n *\n * Second.\n */\nexport const A = 1;\n"
        );
    }

    #[test]
    fn block_comments_get_a_star_on_every_line() {
        let out = written(&Constant::number("B", &["\n * First.\n * Second.\n "], 2u8));
        assert_eq!(
            out,
            "\n/**\n * First.\n * Second.\n */\nexport const B = 2;\n"
        );
    }

    #[test]
    fn a_comment_end_in_the_text_is_escaped() {
        let out = written(&Constant::number("C", &[" Ends with `*/`."], 3u8));
        assert!(out.contains(" * Ends with `*\\/`.\n */\n"), "{out}");
    }

    #[test]
    fn negative_numbers_are_written_and_inexact_ones_refused() {
        assert!(written(&Constant::number("D", &[" Negative."], -5i64)).ends_with("= -5;\n"));
        let too_big = std::panic::catch_unwind(|| Constant::number("E", &[" Big."], u64::MAX));
        assert!(too_big.is_err());
    }

    #[test]
    fn write_typescript_constants() {
        use std::fmt::Write as _;
        let constants = [
            crate::version::exported_consts(),
            crate::exported_consts(),
            crate::wire::exported_consts(),
            crate::frame::exported_consts(),
            crate::app::exported_consts(),
            crate::desktop::exported_consts(),
            crate::version::since_tables(),
            audio_tables(),
            vec![Constant::json(
                "DEFAULT_EXTENSION_SETTINGS",
                &[
                    " The extension's settings on a fresh install (`ExtensionSettings::default`), which",
                    " the extension judges browser downloads by until it first hears from the app.",
                ],
                "ExtensionSettings",
                &["ExtensionSettings"],
                &ExtensionSettings::default(),
            )],
        ];
        let constants: Vec<&Constant> = constants.iter().flatten().collect();
        let mut out = String::from(
            "// Generated from crates/protocol by `pnpm protocol:generate`. Do not edit; run it again after changing the constants there.\n\n",
        );
        let imports: std::collections::BTreeSet<&str> = constants
            .iter()
            .flat_map(|c| c.written.imports.iter().copied())
            .collect();
        for name in imports {
            writeln!(out, "import type {{ {name} }} from \"./{name}\";").unwrap();
        }
        for constant in constants {
            constant.write(&mut out);
        }
        let dir = export_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE_NAME), out).unwrap();
    }
}
