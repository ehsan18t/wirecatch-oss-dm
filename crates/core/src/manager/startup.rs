use wirecatch_product::PRODUCT_NAME;

use wirecatch_protocol::app::StartupNotice;
use wirecatch_store::{KeptValue, Recovery};

/// The notice for a download list that had to be repaired while opening, if it
/// had to be: one notice, saying everything that was repaired.
pub(super) fn list_recovered(recoveries: &[Recovery]) -> Option<StartupNotice> {
    match recoveries {
        [] => None,
        [one] => Some(recovered(one)),
        several => Some(StartupNotice {
            title: format!("{PRODUCT_NAME} repaired its download list"),
            message: several
                .iter()
                .map(|r| recovered(r).message)
                .collect::<Vec<_>>()
                .join(" "),
            set_aside: several.iter().any(|r| recovered(r).set_aside),
        }),
    }
}

/// The notice for a data folder that could not be made private to the person's account: the
/// download list in it holds the cookies websites gave for downloads (D17). The store logs the
/// path and the reason.
pub(super) fn not_private() -> StartupNotice {
    StartupNotice {
        title: "Other accounts may be able to read your download list".to_owned(),
        message: format!(
            "{PRODUCT_NAME} could not make its data folder private to your account, and the list in it keeps the sign-ins websites gave for your downloads. Make sure your account owns the folder, then open {PRODUCT_NAME} again. The log has the folder's location (Settings > General)."
        ),
        set_aside: false,
    }
}

/// The notice for one repair.
fn recovered(recovery: &Recovery) -> StartupNotice {
    match recovery {
        Recovery::Damaged { .. } => StartupNotice {
            title: format!("{PRODUCT_NAME} started a new download list"),
            message: format!(
                "The download list was damaged, so {PRODUCT_NAME} set it aside and started an empty one. Files you already downloaded are still in their folders."
            ),
            set_aside: true,
        },
        Recovery::UnreadableKept(count) => StartupNotice {
            title: "Some downloads are hidden from the list".to_owned(),
            message: format!(
                "{PRODUCT_NAME} could not read {count} {} in its list, perhaps saved by a newer version. {} kept for a version that can read {}, without {} sign-in. Files already downloaded are still in their folders.",
                if *count == 1 { "download" } else { "downloads" },
                if *count == 1 { "It is" } else { "They are" },
                if *count == 1 { "it" } else { "them" },
                if *count == 1 { "its" } else { "their" },
            ),
            set_aside: false,
        },
        Recovery::ValuesReset { downloads } => StartupNotice {
            title: format!("{PRODUCT_NAME} repaired some damaged values"),
            message: format!(
                "{downloads} {} in the list held an impossible size or progress, which {PRODUCT_NAME} reset.",
                if *downloads == 1 {
                    "download"
                } else {
                    "downloads"
                },
            ),
            set_aside: false,
        },
        Recovery::ValueKept(which) => StartupNotice {
            title: format!("{PRODUCT_NAME} could not read part of its list"),
            message: kept_value(*which),
            set_aside: false,
        },
    }
}

/// What not being able to read `which` costs the person (D148).
fn kept_value(which: KeptValue) -> String {
    match which {
        KeptValue::Settings => {
            format!(
                "{PRODUCT_NAME} could not read its settings, perhaps saved by a newer version, so it is using the defaults. Check your folders, limits and proxy in Settings."
            )
        }
        KeptValue::Queue => {
            format!(
                "{PRODUCT_NAME} could not read at least one of its queues, perhaps saved by a newer version. Their downloads moved to the default queue."
            )
        }
        KeptValue::ProxyServers => {
            format!(
                "{PRODUCT_NAME} could not read a saved proxy server. Downloads that used it no longer go through it, so check the proxy in Settings > Network before they run."
            )
        }
        KeptValue::ProxyPasswords => {
            format!(
                "{PRODUCT_NAME} could not read the saved proxy passwords. Enter them again in Settings > Network."
            )
        }
        KeptValue::PendingDeletions => {
            format!(
                "{PRODUCT_NAME} could not read which leftover files of removed downloads it still had to delete, so some may stay in their folders."
            )
        }
        KeptValue::QueueBatches => {
            format!(
                "{PRODUCT_NAME} could not read what each queue had finished so far, so the next summary of a queue may leave some downloads out."
            )
        }
        _ => format!("{PRODUCT_NAME} could not read a saved value and is using its default."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_notices_follow_the_house_style_and_agree_in_number() {
        let recoveries = [
            Recovery::Damaged {
                kept_at: "/data/downloads.db.damaged-1".into(),
            },
            Recovery::UnreadableKept(1),
            Recovery::UnreadableKept(3),
            Recovery::ValuesReset { downloads: 1 },
            Recovery::ValuesReset { downloads: 3 },
            Recovery::ValueKept(KeptValue::Settings),
            Recovery::ValueKept(KeptValue::Queue),
            Recovery::ValueKept(KeptValue::ProxyServers),
            Recovery::ValueKept(KeptValue::ProxyPasswords),
            Recovery::ValueKept(KeptValue::QueueBatches),
        ];
        let damaged = list_recovered(&recoveries[..1]).expect("a notice for the damaged list");
        assert!(
            !damaged.message.contains("damaged-1"),
            "the path breaks the notice's text, and Show the old list reveals it"
        );
        let notices = recoveries
            .iter()
            .map(|r| list_recovered(std::slice::from_ref(r)))
            .chain([list_recovered(&recoveries[1..])]);
        for notice in notices {
            let notice = notice.expect("a repair without a notice");
            for text in [&notice.title, &notice.message] {
                assert!(!text.contains('\u{2014}'), "em dash in {text}");
            }
            assert!(notice.message.ends_with('.'), "{}", notice.message);
        }
        assert_eq!(list_recovered(&[]), None);
        let message = |downloads| {
            list_recovered(&[Recovery::ValuesReset { downloads }]).map_or_default(|n| n.message)
        };
        assert!(message(1).starts_with("1 download in"), "{}", message(1));
        assert!(message(3).starts_with("3 downloads in"), "{}", message(3));
        let open = not_private();
        for text in [&open.title, &open.message] {
            assert!(!text.contains('\u{2014}'), "em dash in {text}");
        }
        assert!(open.message.ends_with('.'), "{}", open.message);
        // Several repairs make one notice that says each.
        let together = list_recovered(&recoveries[1..3]).unwrap().message;
        assert!(together.contains("could not read 1 download"), "{together}");
        assert!(
            together.contains("could not read 3 downloads"),
            "{together}"
        );
    }
}
