//! The subtitle languages a person can pick from: ISO 639-1 codes, each named in English and with
//! the other names people still search it by. One list for the apps and the extension.

use serde::Serialize;

/// A language a person can pick subtitles in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Language {
    /// Its ISO 639-1 code, as sites and yt-dlp name it.
    pub code: &'static str,
    /// Its name in English.
    pub name: &'static str,
    /// Other names people search it by, when it has any.
    pub also_called: Option<&'static str>,
}

/// Every language offered, in the order of their codes.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "aa",
        name: "Afar",
        also_called: None,
    },
    Language {
        code: "ab",
        name: "Abkhazian",
        also_called: None,
    },
    Language {
        code: "af",
        name: "Afrikaans",
        also_called: None,
    },
    Language {
        code: "ak",
        name: "Akan",
        also_called: None,
    },
    Language {
        code: "am",
        name: "Amharic",
        also_called: None,
    },
    Language {
        code: "an",
        name: "Aragonese",
        also_called: None,
    },
    Language {
        code: "ar",
        name: "Arabic",
        also_called: None,
    },
    Language {
        code: "as",
        name: "Assamese",
        also_called: None,
    },
    Language {
        code: "av",
        name: "Avaric",
        also_called: None,
    },
    Language {
        code: "ay",
        name: "Aymara",
        also_called: None,
    },
    Language {
        code: "az",
        name: "Azerbaijani",
        also_called: None,
    },
    Language {
        code: "ba",
        name: "Bashkir",
        also_called: None,
    },
    Language {
        code: "be",
        name: "Belarusian",
        also_called: None,
    },
    Language {
        code: "bg",
        name: "Bulgarian",
        also_called: None,
    },
    Language {
        code: "bi",
        name: "Bislama",
        also_called: None,
    },
    Language {
        code: "bm",
        name: "Bambara",
        also_called: None,
    },
    Language {
        code: "bn",
        name: "Bangla",
        also_called: Some("Bengali"),
    },
    Language {
        code: "bo",
        name: "Tibetan",
        also_called: None,
    },
    Language {
        code: "br",
        name: "Breton",
        also_called: None,
    },
    Language {
        code: "bs",
        name: "Bosnian",
        also_called: None,
    },
    Language {
        code: "ca",
        name: "Catalan",
        also_called: None,
    },
    Language {
        code: "ce",
        name: "Chechen",
        also_called: None,
    },
    Language {
        code: "ch",
        name: "Chamorro",
        also_called: None,
    },
    Language {
        code: "co",
        name: "Corsican",
        also_called: None,
    },
    Language {
        code: "cr",
        name: "Cree",
        also_called: None,
    },
    Language {
        code: "cs",
        name: "Czech",
        also_called: None,
    },
    Language {
        code: "cu",
        name: "Church Slavic",
        also_called: None,
    },
    Language {
        code: "cv",
        name: "Chuvash",
        also_called: None,
    },
    Language {
        code: "cy",
        name: "Welsh",
        also_called: None,
    },
    Language {
        code: "da",
        name: "Danish",
        also_called: None,
    },
    Language {
        code: "de",
        name: "German",
        also_called: None,
    },
    Language {
        code: "dv",
        name: "Divehi",
        also_called: Some("Dhivehi Maldivian"),
    },
    Language {
        code: "dz",
        name: "Dzongkha",
        also_called: None,
    },
    Language {
        code: "ee",
        name: "Ewe",
        also_called: None,
    },
    Language {
        code: "el",
        name: "Greek",
        also_called: None,
    },
    Language {
        code: "en",
        name: "English",
        also_called: None,
    },
    Language {
        code: "eo",
        name: "Esperanto",
        also_called: None,
    },
    Language {
        code: "es",
        name: "Spanish",
        also_called: None,
    },
    Language {
        code: "et",
        name: "Estonian",
        also_called: None,
    },
    Language {
        code: "eu",
        name: "Basque",
        also_called: None,
    },
    Language {
        code: "fa",
        name: "Persian",
        also_called: None,
    },
    Language {
        code: "ff",
        name: "Fula",
        also_called: None,
    },
    Language {
        code: "fi",
        name: "Finnish",
        also_called: None,
    },
    Language {
        code: "fj",
        name: "Fijian",
        also_called: None,
    },
    Language {
        code: "fo",
        name: "Faroese",
        also_called: None,
    },
    Language {
        code: "fr",
        name: "French",
        also_called: None,
    },
    Language {
        code: "fy",
        name: "Western Frisian",
        also_called: None,
    },
    Language {
        code: "ga",
        name: "Irish",
        also_called: None,
    },
    Language {
        code: "gd",
        name: "Scottish Gaelic",
        also_called: None,
    },
    Language {
        code: "gl",
        name: "Galician",
        also_called: None,
    },
    Language {
        code: "gn",
        name: "Guarani",
        also_called: None,
    },
    Language {
        code: "gu",
        name: "Gujarati",
        also_called: None,
    },
    Language {
        code: "gv",
        name: "Manx",
        also_called: None,
    },
    Language {
        code: "ha",
        name: "Hausa",
        also_called: None,
    },
    Language {
        code: "he",
        name: "Hebrew",
        also_called: None,
    },
    Language {
        code: "hi",
        name: "Hindi",
        also_called: None,
    },
    Language {
        code: "ho",
        name: "Hiri Motu",
        also_called: None,
    },
    Language {
        code: "hr",
        name: "Croatian",
        also_called: None,
    },
    Language {
        code: "ht",
        name: "Haitian Creole",
        also_called: None,
    },
    Language {
        code: "hu",
        name: "Hungarian",
        also_called: None,
    },
    Language {
        code: "hy",
        name: "Armenian",
        also_called: None,
    },
    Language {
        code: "hz",
        name: "Herero",
        also_called: None,
    },
    Language {
        code: "ia",
        name: "Interlingua",
        also_called: None,
    },
    Language {
        code: "id",
        name: "Indonesian",
        also_called: None,
    },
    Language {
        code: "ie",
        name: "Interlingue",
        also_called: None,
    },
    Language {
        code: "ig",
        name: "Igbo",
        also_called: None,
    },
    Language {
        code: "ii",
        name: "Sichuan Yi",
        also_called: Some("Nuosu"),
    },
    Language {
        code: "ik",
        name: "Inupiaq",
        also_called: None,
    },
    Language {
        code: "io",
        name: "Ido",
        also_called: None,
    },
    Language {
        code: "is",
        name: "Icelandic",
        also_called: None,
    },
    Language {
        code: "it",
        name: "Italian",
        also_called: None,
    },
    Language {
        code: "iu",
        name: "Inuktitut",
        also_called: None,
    },
    Language {
        code: "ja",
        name: "Japanese",
        also_called: None,
    },
    Language {
        code: "jv",
        name: "Javanese",
        also_called: None,
    },
    Language {
        code: "ka",
        name: "Georgian",
        also_called: None,
    },
    Language {
        code: "kg",
        name: "Kongo",
        also_called: None,
    },
    Language {
        code: "ki",
        name: "Kikuyu",
        also_called: None,
    },
    Language {
        code: "kj",
        name: "Kuanyama",
        also_called: None,
    },
    Language {
        code: "kk",
        name: "Kazakh",
        also_called: None,
    },
    Language {
        code: "kl",
        name: "Kalaallisut",
        also_called: Some("Greenlandic"),
    },
    Language {
        code: "km",
        name: "Khmer",
        also_called: None,
    },
    Language {
        code: "kn",
        name: "Kannada",
        also_called: None,
    },
    Language {
        code: "ko",
        name: "Korean",
        also_called: None,
    },
    Language {
        code: "kr",
        name: "Kanuri",
        also_called: None,
    },
    Language {
        code: "ks",
        name: "Kashmiri",
        also_called: None,
    },
    Language {
        code: "ku",
        name: "Kurdish",
        also_called: None,
    },
    Language {
        code: "kv",
        name: "Komi",
        also_called: None,
    },
    Language {
        code: "kw",
        name: "Cornish",
        also_called: None,
    },
    Language {
        code: "ky",
        name: "Kyrgyz",
        also_called: Some("Kirghiz"),
    },
    Language {
        code: "la",
        name: "Latin",
        also_called: None,
    },
    Language {
        code: "lb",
        name: "Luxembourgish",
        also_called: None,
    },
    Language {
        code: "lg",
        name: "Ganda",
        also_called: None,
    },
    Language {
        code: "li",
        name: "Limburgish",
        also_called: None,
    },
    Language {
        code: "ln",
        name: "Lingala",
        also_called: None,
    },
    Language {
        code: "lo",
        name: "Lao",
        also_called: None,
    },
    Language {
        code: "lt",
        name: "Lithuanian",
        also_called: None,
    },
    Language {
        code: "lu",
        name: "Luba-Katanga",
        also_called: None,
    },
    Language {
        code: "lv",
        name: "Latvian",
        also_called: None,
    },
    Language {
        code: "mg",
        name: "Malagasy",
        also_called: None,
    },
    Language {
        code: "mh",
        name: "Marshallese",
        also_called: None,
    },
    Language {
        code: "mi",
        name: "Māori",
        also_called: None,
    },
    Language {
        code: "mk",
        name: "Macedonian",
        also_called: None,
    },
    Language {
        code: "ml",
        name: "Malayalam",
        also_called: None,
    },
    Language {
        code: "mn",
        name: "Mongolian",
        also_called: None,
    },
    Language {
        code: "mr",
        name: "Marathi",
        also_called: None,
    },
    Language {
        code: "ms",
        name: "Malay",
        also_called: None,
    },
    Language {
        code: "mt",
        name: "Maltese",
        also_called: None,
    },
    Language {
        code: "my",
        name: "Burmese",
        also_called: None,
    },
    Language {
        code: "na",
        name: "Nauru",
        also_called: None,
    },
    Language {
        code: "nb",
        name: "Norwegian Bokmål",
        also_called: None,
    },
    Language {
        code: "nd",
        name: "North Ndebele",
        also_called: None,
    },
    Language {
        code: "ne",
        name: "Nepali",
        also_called: None,
    },
    Language {
        code: "ng",
        name: "Ndonga",
        also_called: None,
    },
    Language {
        code: "nl",
        name: "Dutch",
        also_called: None,
    },
    Language {
        code: "nn",
        name: "Norwegian Nynorsk",
        also_called: None,
    },
    Language {
        code: "no",
        name: "Norwegian",
        also_called: None,
    },
    Language {
        code: "nr",
        name: "South Ndebele",
        also_called: None,
    },
    Language {
        code: "nv",
        name: "Navajo",
        also_called: None,
    },
    Language {
        code: "ny",
        name: "Nyanja",
        also_called: Some("Chichewa Chewa"),
    },
    Language {
        code: "oc",
        name: "Occitan",
        also_called: None,
    },
    Language {
        code: "oj",
        name: "Ojibwa",
        also_called: None,
    },
    Language {
        code: "om",
        name: "Oromo",
        also_called: None,
    },
    Language {
        code: "or",
        name: "Odia",
        also_called: Some("Oriya"),
    },
    Language {
        code: "os",
        name: "Ossetic",
        also_called: Some("Ossetic"),
    },
    Language {
        code: "pa",
        name: "Punjabi",
        also_called: Some("Panjabi"),
    },
    Language {
        code: "pi",
        name: "Pali",
        also_called: None,
    },
    Language {
        code: "pl",
        name: "Polish",
        also_called: None,
    },
    Language {
        code: "ps",
        name: "Pashto",
        also_called: None,
    },
    Language {
        code: "pt",
        name: "Portuguese",
        also_called: None,
    },
    Language {
        code: "qu",
        name: "Quechua",
        also_called: None,
    },
    Language {
        code: "rm",
        name: "Romansh",
        also_called: None,
    },
    Language {
        code: "rn",
        name: "Rundi",
        also_called: None,
    },
    Language {
        code: "ro",
        name: "Romanian",
        also_called: None,
    },
    Language {
        code: "ru",
        name: "Russian",
        also_called: None,
    },
    Language {
        code: "rw",
        name: "Kinyarwanda",
        also_called: None,
    },
    Language {
        code: "sa",
        name: "Sanskrit",
        also_called: None,
    },
    Language {
        code: "sc",
        name: "Sardinian",
        also_called: None,
    },
    Language {
        code: "sd",
        name: "Sindhi",
        also_called: None,
    },
    Language {
        code: "se",
        name: "Northern Sami",
        also_called: None,
    },
    Language {
        code: "sg",
        name: "Sango",
        also_called: None,
    },
    Language {
        code: "si",
        name: "Sinhala",
        also_called: None,
    },
    Language {
        code: "sk",
        name: "Slovak",
        also_called: None,
    },
    Language {
        code: "sl",
        name: "Slovenian",
        also_called: None,
    },
    Language {
        code: "sm",
        name: "Samoan",
        also_called: None,
    },
    Language {
        code: "sn",
        name: "Shona",
        also_called: None,
    },
    Language {
        code: "so",
        name: "Somali",
        also_called: None,
    },
    Language {
        code: "sq",
        name: "Albanian",
        also_called: None,
    },
    Language {
        code: "sr",
        name: "Serbian",
        also_called: None,
    },
    Language {
        code: "ss",
        name: "Swati",
        also_called: None,
    },
    Language {
        code: "st",
        name: "Southern Sotho",
        also_called: None,
    },
    Language {
        code: "su",
        name: "Sundanese",
        also_called: None,
    },
    Language {
        code: "sv",
        name: "Swedish",
        also_called: None,
    },
    Language {
        code: "sw",
        name: "Swahili",
        also_called: None,
    },
    Language {
        code: "ta",
        name: "Tamil",
        also_called: None,
    },
    Language {
        code: "te",
        name: "Telugu",
        also_called: None,
    },
    Language {
        code: "tg",
        name: "Tajik",
        also_called: None,
    },
    Language {
        code: "th",
        name: "Thai",
        also_called: None,
    },
    Language {
        code: "ti",
        name: "Tigrinya",
        also_called: None,
    },
    Language {
        code: "tk",
        name: "Turkmen",
        also_called: None,
    },
    Language {
        code: "tl",
        name: "Filipino",
        also_called: None,
    },
    Language {
        code: "tn",
        name: "Tswana",
        also_called: None,
    },
    Language {
        code: "to",
        name: "Tongan",
        also_called: None,
    },
    Language {
        code: "tr",
        name: "Turkish",
        also_called: None,
    },
    Language {
        code: "ts",
        name: "Tsonga",
        also_called: None,
    },
    Language {
        code: "tt",
        name: "Tatar",
        also_called: None,
    },
    Language {
        code: "tw",
        name: "Akan",
        also_called: None,
    },
    Language {
        code: "ty",
        name: "Tahitian",
        also_called: None,
    },
    Language {
        code: "ug",
        name: "Uyghur",
        also_called: Some("Uighur"),
    },
    Language {
        code: "uk",
        name: "Ukrainian",
        also_called: None,
    },
    Language {
        code: "ur",
        name: "Urdu",
        also_called: None,
    },
    Language {
        code: "uz",
        name: "Uzbek",
        also_called: None,
    },
    Language {
        code: "ve",
        name: "Venda",
        also_called: None,
    },
    Language {
        code: "vi",
        name: "Vietnamese",
        also_called: None,
    },
    Language {
        code: "vo",
        name: "Volapük",
        also_called: None,
    },
    Language {
        code: "wa",
        name: "Walloon",
        also_called: None,
    },
    Language {
        code: "wo",
        name: "Wolof",
        also_called: None,
    },
    Language {
        code: "xh",
        name: "Xhosa",
        also_called: None,
    },
    Language {
        code: "yi",
        name: "Yiddish",
        also_called: None,
    },
    Language {
        code: "yo",
        name: "Yoruba",
        also_called: None,
    },
    Language {
        code: "za",
        name: "Zhuang",
        also_called: None,
    },
    Language {
        code: "zh",
        name: "Chinese",
        also_called: None,
    },
    Language {
        code: "zu",
        name: "Zulu",
        also_called: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_code_is_listed_once_in_order() {
        assert!(LANGUAGES.windows(2).all(|pair| pair[0].code < pair[1].code));
    }
}
