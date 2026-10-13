//! Settings changed from several windows at once. A window used to send its whole copy of the settings, so a change made in another window that had not reached it yet was quietly undone.

use wirecatch_protocol::app::{
    AppSettingsPatch, MediaDefaultsPatch, OutputDefaultsPatch, Theme, Update,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;

use crate::common::{Harness, TestResult};

#[tokio::test]
async fn two_windows_changing_different_settings_keep_both() -> TestResult {
    let h = Harness::new().await?;
    let before = h.manager.get_settings();

    // Both windows start from the same copy; each changes one setting.
    h.manager
        .change_settings(AppSettingsPatch {
            theme: Some(Theme::Dark),
            ..AppSettingsPatch::default()
        })
        .unwrap();
    h.manager
        .change_settings(AppSettingsPatch {
            extension: Some(ExtensionSettingsPatch {
                ask_before_download: Some(!before.extension.ask_before_download),
                ..ExtensionSettingsPatch::default()
            }),
            ..AppSettingsPatch::default()
        })
        .unwrap();

    let after = h.manager.get_settings();
    assert_eq!(
        after.theme,
        Theme::Dark,
        "the first window's change survives the second"
    );
    assert_eq!(
        after.extension.ask_before_download,
        !before.extension.ask_before_download
    );
    assert_eq!(
        after.extension.capture_enabled, before.extension.capture_enabled,
        "the rest of the group is untouched"
    );
    Ok(())
}

// A setting that can be empty (no speed limit, the format's usual audio quality) must be able to
// go from empty to a value and back, which "leave unchanged" alone cannot say.
#[tokio::test]
async fn a_setting_that_can_be_empty_is_set_and_cleared() -> TestResult {
    let h = Harness::new().await?;
    let limit = |limit: Option<u64>| AppSettingsPatch {
        global_speed_limit: Update::Set(limit),
        ..AppSettingsPatch::default()
    };
    let kbps = |kbps: Option<u32>| AppSettingsPatch {
        media: Some(MediaDefaultsPatch {
            audio: Some(OutputDefaultsPatch {
                audio_kbps: Update::Set(kbps),
                ..OutputDefaultsPatch::default()
            }),
            ..MediaDefaultsPatch::default()
        }),
        ..AppSettingsPatch::default()
    };

    h.manager.change_settings(limit(Some(500_000))).unwrap();
    h.manager.change_settings(kbps(Some(192))).unwrap();
    let set = h.manager.get_settings();
    assert_eq!(set.global_speed_limit, Some(500_000));
    assert_eq!(set.media.audio.audio_kbps, Some(192));

    h.manager
        .change_settings(AppSettingsPatch::default())
        .unwrap();
    assert_eq!(
        h.manager.get_settings(),
        set,
        "an empty patch changes nothing"
    );

    h.manager.change_settings(limit(None)).unwrap();
    h.manager.change_settings(kbps(None)).unwrap();
    let cleared = h.manager.get_settings();
    assert_eq!(cleared.global_speed_limit, None);
    assert_eq!(cleared.media.audio.audio_kbps, None);
    Ok(())
}

#[tokio::test]
async fn a_refused_patch_changes_nothing() -> TestResult {
    let h = Harness::new().await?;
    let before = h.manager.get_settings();

    let refused = h.manager.change_settings(AppSettingsPatch {
        theme: Some(Theme::Dark),
        max_concurrent_downloads: Some(0),
        ..AppSettingsPatch::default()
    });

    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(h.manager.get_settings(), before);
    Ok(())
}

fn sites(list: impl IntoIterator<Item = String>) -> Vec<String> {
    list.into_iter().collect()
}

/// About 80,000 sites fit in one frame. Such a list is refused for its length before any site in it is read, from the browser and from a window, and changes nothing: its first entry names no host, and the refusal is the one for the length. That it is refused without the manager's lock is held by `manager::tests::a_list_of_sites_too_long_is_refused_without_the_lock`.
#[tokio::test]
async fn a_frame_full_of_sites_is_refused_unread_from_the_browser_and_the_windows() -> TestResult {
    let h = Harness::new().await?;
    let before = h.manager.get_settings();
    let flood = || {
        let no_host = std::iter::once("exa mple.com".to_owned());
        Some(sites(
            no_host.chain((0..80_000).map(|i| format!("s{i}.example"))),
        ))
    };

    let from_browser = h.manager.update_extension_settings(ExtensionSettingsPatch {
        add_ignored_sites: flood(),
        ..ExtensionSettingsPatch::default()
    });
    let from_window = h.manager.change_settings(AppSettingsPatch {
        extension: Some(ExtensionSettingsPatch {
            ignored_sites: flood(),
            ..ExtensionSettingsPatch::default()
        }),
        ..AppSettingsPatch::default()
    });

    for refused in [from_browser.unwrap_err(), from_window.unwrap_err()] {
        let refused = refused.to_string();
        assert!(refused.contains("at most 1000"), "{refused}");
    }
    assert_eq!(h.manager.get_settings(), before);
    Ok(())
}

/// A site that names no host is refused with the entry named, so the person sees which one; a blank entry is left out.
#[tokio::test]
async fn a_site_that_names_no_host_is_refused_by_name() -> TestResult {
    let h = Harness::new().await?;
    let before = h.manager.get_settings();

    let refused = h
        .manager
        .update_extension_settings(ExtensionSettingsPatch {
            add_ignored_sites: Some(sites(["a.example".into(), "exa mple.com".into()])),
            ..ExtensionSettingsPatch::default()
        })
        .unwrap_err()
        .to_string();
    assert!(refused.contains("\"exa mple.com\""), "{refused}");
    assert_eq!(h.manager.get_settings(), before);

    let kept = h
        .manager
        .update_extension_settings(ExtensionSettingsPatch {
            add_ignored_sites: Some(sites(["A.example/".into(), " ".into()])),
            remove_ignored_sites: Some(sites(["not a site".into()])),
            ..ExtensionSettingsPatch::default()
        })
        .unwrap();
    assert_eq!(kept.ignored_sites, ["a.example"]);
    Ok(())
}
