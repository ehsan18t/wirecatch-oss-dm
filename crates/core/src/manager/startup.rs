use std::path::Path;

use wirecatch_protocol::app::StartupNotice;

use crate::store::Recovery;

/// The notice for a download list that had to be repaired while opening, if it
/// had to be: one notice, saying everything that was repaired.
pub(super) fn list_recovered(recoveries: &[Recovery]) -> Option<StartupNotice> {
    match recoveries {
        [] => None,
        [one] => Some(recovered(one)),
        several => Some(StartupNotice {
            title: "Wirecatch repaired its download list".to_owned(),
            message: several
                .iter()
                .map(|r| recovered(r).message)
                .collect::<Vec<_>>()
                .join(" "),
            set_aside: several.iter().any(|r| recovered(r).set_aside),
        }),
    }
}

/// The notice for a data folder that could not be made private to the person's account, for
/// `reason`: the download list in it holds the cookies websites gave for downloads (D17).
pub(super) fn not_private(folder: &Path, reason: &str) -> StartupNotice {
    StartupNotice {
        title: "Other accounts may be able to read your download list".to_owned(),
        message: format!(
            "Wirecatch could not make its folder {} private to your account ({reason}). The download list in it keeps the sign-ins websites gave for your downloads, so other accounts on this computer may be able to read them. Make sure your account owns the folder and that it is on a drive that keeps permissions, then open Wirecatch again.",
            folder.display()
        ),
        set_aside: false,
    }
}

/// The notice for one repair.
fn recovered(recovery: &Recovery) -> StartupNotice {
    match recovery {
        Recovery::Damaged { kept_at } => StartupNotice {
            title: "Wirecatch started a new download list".to_owned(),
            message: format!(
                "The download list was damaged, so Wirecatch set it aside and started an empty one. Files you already downloaded are still in their folders. The damaged list is kept at {}.",
                kept_at.display()
            ),
            set_aside: true,
        },
        Recovery::UnreadableKept(count) => StartupNotice {
            title: "Some downloads are hidden from the list".to_owned(),
            message: format!(
                "Wirecatch could not read {count} {} in its list, perhaps saved by a newer version. {} kept for a version that can read {}, without {} sign-in. Files already downloaded are still in their folders.",
                if *count == 1 { "download" } else { "downloads" },
                if *count == 1 { "It is" } else { "They are" },
                if *count == 1 { "it" } else { "them" },
                if *count == 1 { "its" } else { "their" },
            ),
            set_aside: false,
        },
        Recovery::ValuesReset { downloads } => StartupNotice {
            title: "Wirecatch repaired some damaged values".to_owned(),
            message: format!(
                "{downloads} {} in the list held an impossible size or progress, which Wirecatch reset.",
                if *downloads == 1 {
                    "download"
                } else {
                    "downloads"
                },
            ),
            set_aside: false,
        },
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
        ];
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
            list_recovered(&[Recovery::ValuesReset { downloads }])
                .map(|n| n.message)
                .unwrap_or_default()
        };
        assert!(message(1).starts_with("1 download in"), "{}", message(1));
        assert!(message(3).starts_with("3 downloads in"), "{}", message(3));
        let open = not_private(Path::new("/data"), "access denied");
        for text in [&open.title, &open.message] {
            assert!(!text.contains('\u{2014}'), "em dash in {text}");
        }
        assert!(open.message.ends_with('.'), "{}", open.message);
        assert!(open.message.contains("/data"), "{}", open.message);
        // Several repairs make one notice that says each.
        let together = list_recovered(&recoveries[1..3]).unwrap().message;
        assert!(together.contains("could not read 1 download"), "{together}");
        assert!(
            together.contains("could not read 3 downloads"),
            "{together}"
        );
    }
}
