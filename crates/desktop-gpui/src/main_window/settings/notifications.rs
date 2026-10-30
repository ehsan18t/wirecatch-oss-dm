//! Notifications: how finished, failed and started downloads are told (D52, D64, D65).

use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_kit::{App, Entity};
use wirecatch_protocol::app::{
    AppSettingsPatch, FinishNoticeTime, NotifyOnFinish, NotifyOnStart, StartNoticeTime,
};

use super::{choice, current, switch};
use crate::library::Library;

const ON_FINISH: &[(NotifyOnFinish, &str)] = &[
    (NotifyOnFinish::Notification, "Show a notification"),
    (NotifyOnFinish::Browser, "Show a notice in the browser"),
    (NotifyOnFinish::Window, "Open a summary window"),
    (NotifyOnFinish::Nothing, "Don\u{2019}t notify"),
];

const FINISH_TIMES: &[(FinishNoticeTime, &str)] = &[
    (FinishNoticeTime::FiveSeconds, "5 seconds"),
    (FinishNoticeTime::EightSeconds, "8 seconds"),
    (FinishNoticeTime::FifteenSeconds, "15 seconds"),
    (FinishNoticeTime::UntilClosed, "Until I close it"),
];

const ON_START: &[(NotifyOnStart, &str)] = &[
    (NotifyOnStart::Browser, "Show a notice in the browser"),
    (NotifyOnStart::Nothing, "Don\u{2019}t notify"),
];

const START_TIMES: &[(StartNoticeTime, &str)] = &[
    (StartNoticeTime::ThreeSeconds, "3 seconds"),
    (StartNoticeTime::FiveSeconds, "5 seconds"),
    (StartNoticeTime::EightSeconds, "8 seconds"),
    (StartNoticeTime::TenSeconds, "10 seconds"),
];

/// Said under a notice's time while no notice is shown in the browser (D85.10).
const ONLY_IN_BROWSER: &str = "Used only with Show a notice in the browser.";

pub(super) fn page(library: &Entity<Library>, cx: &App) -> SettingPage {
    SettingPage::new("Notifications").resettable(false).groups([
        finished(library, cx),
        started(library, cx),
        do_not_disturb(library),
    ])
}

fn finished(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let on_finish = current(
        library,
        cx,
        |s| s.notify_on_finish,
        NotifyOnFinish::default(),
    );
    SettingGroup::new()
        .title("Finished downloads")
        .item(
            SettingItem::new(
                "When a download finishes",
                choice(
                    library,
                    ON_FINISH,
                    |s| s.notify_on_finish,
                    |value| AppSettingsPatch {
                        notify_on_finish: Some(value),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description("Failed downloads are told the same way, right away.")
            .keywords([
                "notification",
                "alert",
                "done",
                "complete",
                "failed",
                "toast",
            ]),
        )
        .item(
            SettingItem::new(
                "Hide the notice after",
                choice(
                    library,
                    FINISH_TIMES,
                    |s| s.finish_notice_time,
                    |value| AppSettingsPatch {
                        finish_notice_time: Some(value),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .browser_only(
                on_finish == NotifyOnFinish::Browser,
                "Notices about failed downloads stay until you close them.",
            )
            .keywords(["seconds", "duration", "browser"]),
        )
}

fn started(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let on_start = current(library, cx, |s| s.notify_on_start, NotifyOnStart::default());
    SettingGroup::new()
        .title("Started downloads")
        .description("Browser downloads that start without a window.")
        .item(
            SettingItem::new(
                "When a download starts",
                choice(library, ON_START, |s| s.notify_on_start, |value| {
                    AppSettingsPatch {
                        notify_on_start: Some(value),
                        ..AppSettingsPatch::default()
                    }
                }),
            )
            .description(
                "A notice in the current tab, or a system notification while the browser is in the background.",
            )
            .keywords(["started", "begin", "notice", "browser", "tab"]),
        )
        .item(
            SettingItem::new(
                "Hide the notice after",
                choice(library, START_TIMES, |s| s.start_notice_time, |value| {
                    AppSettingsPatch {
                        start_notice_time: Some(value),
                        ..AppSettingsPatch::default()
                    }
                }),
            )
            .browser_only(
                on_start == NotifyOnStart::Browser,
                "Also how long its system notification stays.",
            )
            .keywords(["seconds", "duration", "browser"]),
        )
}

fn do_not_disturb(library: &Entity<Library>) -> SettingGroup {
    SettingGroup::new().item(
        SettingItem::new(
            "Show notifications during Do Not Disturb",
            switch(library, |s| s.notify_during_do_not_disturb, |on| {
                AppSettingsPatch {
                    notify_during_do_not_disturb: Some(on),
                    ..AppSettingsPatch::default()
                }
            }),
        )
        .description(
            "Finished and failed downloads and problems still notify you. The system can still block them in its own settings.",
        )
        .keywords(["dnd", "focus", "quiet", "silent", "mute"]),
    )
}

trait BrowserOnly {
    /// Greys out a row that does nothing without a notice in the browser, and says so (D85.10).
    fn browser_only(self, shown: bool, help: &'static str) -> Self;
}

impl BrowserOnly for SettingItem {
    fn browser_only(self, shown: bool, help: &'static str) -> Self {
        self.description(if shown { help } else { ONLY_IN_BROWSER })
            .disabled(!shown)
    }
}
