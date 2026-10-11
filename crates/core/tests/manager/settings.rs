//! Settings changed from several windows at once. A window used to send its whole copy of the settings, so a change made in another window that had not reached it yet was quietly undone.

use wirecatch_protocol::app::{
    AppSettingsPatch, MediaDefaultsPatch, OutputDefaultsPatch, Theme, Update,
};
use wirecatch_protocol::extension::ExtensionSettingsPatch;

use crate::common::Harness;

#[tokio::test]
async fn two_windows_changing_different_settings_keep_both() {
    let h = Harness::new().await;
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
}

// A setting that can be empty (no speed limit, the format's usual audio quality) must be able to
// go from empty to a value and back, which "leave unchanged" alone cannot say.
#[tokio::test]
async fn a_setting_that_can_be_empty_is_set_and_cleared() {
    let h = Harness::new().await;
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
}

#[tokio::test]
async fn a_refused_patch_changes_nothing() {
    let h = Harness::new().await;
    let before = h.manager.get_settings();

    let refused = h.manager.change_settings(AppSettingsPatch {
        theme: Some(Theme::Dark),
        max_concurrent_downloads: Some(0),
        ..AppSettingsPatch::default()
    });

    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(h.manager.get_settings(), before);
}
