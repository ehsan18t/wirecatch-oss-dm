//! Settings changed from several windows at once. A window used to send its whole copy of the settings, so a change made in another window that had not reached it yet was quietly undone.

use serde_json::json;
use wirecatch_core::CoreError;
use wirecatch_protocol::app::{SettingsChange, Theme};

use crate::common::Harness;

fn change(path: &[&str], value: serde_json::Value) -> SettingsChange {
    SettingsChange {
        path: path.iter().map(|name| (*name).to_owned()).collect(),
        value,
    }
}

#[tokio::test]
async fn two_windows_changing_different_settings_keep_both() {
    let h = Harness::new().await;
    let before = h.manager.get_settings();

    // Both windows start from the same copy; each changes one setting.
    h.manager
        .change_settings(&[change(&["theme"], json!("dark"))])
        .unwrap();
    h.manager
        .change_settings(&[change(
            &["extension", "ask_before_download"],
            json!(!before.extension.ask_before_download),
        )])
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

#[tokio::test]
async fn an_unknown_setting_is_refused_and_changes_nothing() {
    let h = Harness::new().await;
    let before = h.manager.get_settings();

    let refused = h.manager.change_settings(&[
        change(&["theme"], json!("dark")),
        change(&["no_such_setting"], json!(1)),
    ]);

    assert!(
        matches!(refused, Err(CoreError::SettingsChange { .. })),
        "{refused:?}"
    );
    assert_eq!(
        h.manager.get_settings(),
        before,
        "a refused batch changes nothing"
    );
}

#[tokio::test]
async fn a_value_of_the_wrong_kind_is_refused() {
    let h = Harness::new().await;
    let before = h.manager.get_settings();

    let refused = h.manager.change_settings(&[change(&["theme"], json!(42))]);

    assert!(
        matches!(refused, Err(CoreError::SettingsChange { .. })),
        "{refused:?}"
    );
    assert_eq!(h.manager.get_settings(), before);
}

#[tokio::test]
async fn a_path_into_a_value_that_is_not_a_group_is_refused() {
    let h = Harness::new().await;

    let refused = h
        .manager
        .change_settings(&[change(&["theme", "inner"], json!(1))]);

    assert!(
        matches!(refused, Err(CoreError::SettingsChange { .. })),
        "{refused:?}"
    );
}
