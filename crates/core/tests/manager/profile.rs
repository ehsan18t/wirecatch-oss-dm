//! Acceptance 5: settings exported on one machine and imported on another (with a
//! different home folder) come back with folders resolved for the new machine.

use wirecatch_core::{CoreError, DEFAULT_QUEUE_ID};
use wirecatch_protocol::app::{Category, PortablePath, Queue, Schedule, TimeOfDay, UiEvent};

use crate::common::{Harness, tune};

fn absolute(path: std::path::PathBuf) -> PortablePath {
    PortablePath(path.to_string_lossy().into_owned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_and_queues_move_to_another_machine() {
    let here = Harness::new().await;
    let night = here.manager.create_queue("Night").unwrap();
    let night = here
        .manager
        .update_queue(Queue {
            max_concurrent: 4,
            speed_limit: Some(500_000),
            schedule: Some(Schedule {
                start: TimeOfDay { hour: 1, minute: 0 },
                stop: Some(TimeOfDay {
                    hour: 6,
                    minute: 30,
                }),
                days: vec![],
            }),
            ..night
        })
        .unwrap();
    let downloads = here.folders.downloads.clone();
    let videos = here.folders.videos.clone();
    tune(&here.manager, |s| {
        s.default_folder = absolute(downloads.join("Stuff"));
        let video = s
            .categories
            .iter_mut()
            .find(|r| r.category == Category::Video)
            .unwrap();
        video.folder = absolute(videos.join("Clips"));
        s.global_speed_limit = Some(2_000_000);
        s.extension.ask_before_download = true;
        s.extension.default_queue = night.id;
    });

    let file = here.root.path().join("profile.json");
    here.manager.export_settings(&file).await.unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("{Downloads}/Stuff"), "{text}");
    assert!(text.contains("{Videos}/Clips"), "{text}");
    let home = here.folders.home.to_string_lossy().replace('\\', "\\\\");
    assert!(
        !text.contains(&home),
        "an absolute path leaked into the profile"
    );

    // A different user on a different machine.
    let there = Harness::new().await;
    assert_ne!(there.folders.home, here.folders.home);
    let mut events = there.manager.subscribe();
    there.manager.import_settings(&file).await.unwrap();

    let settings = there.manager.get_settings();
    assert_eq!(
        settings.default_folder,
        PortablePath("{Downloads}/Stuff".into())
    );
    assert_eq!(settings.global_speed_limit, Some(2_000_000));
    assert!(settings.extension.ask_before_download);
    let queues = there.manager.list_queues();
    assert_eq!(queues.len(), 2);
    let imported = queues.iter().find(|q| q.name == "Night").unwrap();
    assert_eq!(
        (
            imported.max_concurrent,
            imported.speed_limit,
            &imported.schedule
        ),
        (4, Some(500_000), &night.schedule)
    );
    assert_eq!(settings.extension.default_queue, imported.id);

    // New downloads land in this machine's folders.
    let movie = there
        .add_with("https://example.com/movie.mp4", |r| r.start = false)
        .await;
    assert_eq!(
        std::path::Path::new(&movie.path),
        there.folders.videos.join("Clips").join("movie.mp4")
    );
    let other = there
        .add_with("https://example.com/notes.bin", |r| r.start = false)
        .await;
    assert_eq!(
        std::path::Path::new(&other.path),
        there.folders.downloads.join("Stuff").join("notes.bin")
    );

    // The UI heard about both changes, and got a summary to show.
    let mut saw = (false, false);
    let mut summary = None;
    while let Ok(event) = events.try_recv() {
        match event {
            UiEvent::QueuesChanged(_) => saw.0 = true,
            UiEvent::SettingsChanged(_) => saw.1 = true,
            UiEvent::SettingsImported(s) => summary = Some(s),
            _ => {}
        }
    }
    assert_eq!(saw, (true, true));
    let summary = summary.expect("no import summary");
    assert_eq!((summary.queues_added, summary.queues_updated), (1, 1));
    let default = &summary.folders[0];
    assert_eq!(default.setting, "Default folder");
    assert_eq!(default.used, PortablePath("{Downloads}/Stuff".into()));
    assert_eq!(
        std::path::Path::new(&default.resolved),
        there.folders.downloads.join("Stuff")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_profile_from_a_newer_version_is_refused() {
    let h = Harness::new().await;
    let file = h.root.path().join("future.json");
    std::fs::write(&file, r#"{"format_version": 99, "settings": "who knows"}"#).unwrap();
    let before = h.manager.get_settings();
    let err = h.manager.import_settings(&file).await.unwrap_err();
    assert!(matches!(err, CoreError::ProfileTooNew { found: 99, .. }));
    assert!(err.to_string().contains("newer version of Wirecatch"));
    assert_eq!(h.manager.get_settings(), before);
    assert_eq!(h.manager.list_queues().len(), 1);
    assert_eq!(h.manager.list_queues()[0].id, DEFAULT_QUEUE_ID);
}
