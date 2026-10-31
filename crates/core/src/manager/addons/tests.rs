use wirecatch_addons::InstalledComponent;
use wirecatch_addons::archive;
use wirecatch_addons::manifest::{self, ReleaseFile};
use wirecatch_addons::manifest::{ArchiveKind, Sha256Digest};
use wirecatch_engine::{DownloadOptions, Url};

use super::install::{file_safe, fits, stage_folder, swap_in, verify_and_unpack};
use super::*;

/// Add-on downloads name the app, never the engine library's default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_on_downloads_name_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let folders = crate::KnownFolders::under_home(dir.path().join("home"));
    let config = crate::ManagerConfig::for_tests(dir.path().join("data"), folders);
    let manager = Manager::open(config).await.unwrap();
    let agent = manager.addon_download_options().user_agent;
    assert_eq!(
        agent,
        crate::headers::download_options(
            PrivateNetwork::Allow,
            wirecatch_engine::ProxyChoice::System
        )
        .user_agent
    );
    assert_ne!(agent, DownloadOptions::default().user_agent);
}

/// A quiet or stopped install is not shown when it fails, unless it lost a
/// part: the add-on then no longer works, and the person must hear why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_install_that_loses_a_part_says_so_even_quiet_or_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let folders = crate::KnownFolders::under_home(dir.path().join("home"));
    let config = crate::ManagerConfig::for_tests(dir.path().join("data"), folders);
    let manager = Manager::open(config).await.unwrap();
    let end_with = |part_lost: bool, cancelled: bool| {
        manager.lock().addon.op = Some(AddonOp {
            cancel: CancellationToken::new(),
            ended: CancellationToken::new(),
            done: 0,
            total: 0,
            kind: OpKind::Quiet,
        });
        let failure = InstallFailure {
            reason: "ffmpeg could not be put in place.".into(),
            part_lost,
            refused: None,
        };
        manager.end_addon_op(Err(failure), cancelled);
        manager.lock().addon.failure.take()
    };
    assert_eq!(end_with(false, false), None, "a quiet failure was shown");
    assert_eq!(end_with(false, true), None, "a stopped install was shown");
    for cancelled in [false, true] {
        let shown = end_with(true, cancelled).expect("the lost part was not shown");
        assert!(
            shown.contains("ffmpeg could not be put in place."),
            "{shown}"
        );
    }
}

/// A part with one program file, `tool`, from release file `sha256`.
fn part(id: ComponentId, sha256: Sha256Digest) -> AddonComponent {
    AddonComponent::new(
        id,
        "2",
        ReleaseFile::new(Url::parse("https://example.com/tool").unwrap(), sha256, 4).unwrap(),
        ArchiveKind::Plain,
        vec![manifest::AddonFile {
            from: String::new(),
            to: "tool".into(),
            executable: true,
        }],
        "tool",
    )
    .unwrap()
}

/// A new part that cannot be put in place, when the old one cannot be put
/// back either, leaves the add-on without that part: the record must stop
/// claiming it, and the error must say to install the add-on again.
#[test]
fn a_part_that_cannot_be_put_back_is_no_longer_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join(WORK_FOLDER);
    std::fs::create_dir_all(&work).unwrap();
    let target = dir.path().join("ffmpeg");
    std::fs::create_dir_all(&target).unwrap();
    let mut record = Record::default();
    record.components.insert(
        ComponentId::Ffmpeg,
        InstalledComponent {
            version: "1".into(),
            program: "ffmpeg".into(),
            files: Vec::new(),
        },
    );
    record.save(dir.path()).unwrap();
    let component = part(ComponentId::Ffmpeg, Sha256Digest::from_bytes([0; 32]));
    let staged = vec![(
        component,
        (work.join(stage_folder(ComponentId::Ffmpeg)), Vec::new()),
    )];
    let mut calls = 0;
    let failure = swap_in(dir.path(), &work, record, staged, |from, to| {
        calls += 1;
        // Moving the old part aside works; putting the new one in place
        // and the old one back both fail.
        if calls == 1 {
            std::fs::rename(from, to)
        } else {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    })
    .unwrap_err();
    assert_eq!(calls, 3);
    assert!(failure.part_lost);
    let reason = failure.reason;
    assert!(
        reason.contains("Install the media add-on again"),
        "{reason}"
    );
    assert!(
        !Record::load(dir.path())
            .unwrap()
            .components
            .contains_key(&ComponentId::Ffmpeg),
        "the record still claims the part"
    );
}

/// A checksum is a value, not its spelling: upper-case hex names the same file.
#[test]
fn a_checksum_given_in_upper_case_still_matches() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    std::fs::write(&release, b"tool").unwrap();
    let upper = archive::sha256_file(&release)
        .unwrap()
        .to_string()
        .to_ascii_uppercase();
    let component = part(ComponentId::Deno, Sha256Digest::parse(&upper).unwrap());
    let (_, installed) =
        verify_and_unpack(&component, &release, &dir.path().join("stage"), 100).unwrap();
    assert_eq!(installed.len(), 1);
}

/// A part's folder is named by the part, so nothing a caller names can move or delete a folder outside the add-on's: each part lands in its own folder inside it.
#[test]
fn a_part_cannot_be_put_outside_the_add_on_folder() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("media");
    let work = dir.join(WORK_FOLDER);
    let mut staged = Vec::new();
    for id in ComponentId::ALL {
        let stage = work.join(stage_folder(id));
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("tool"), b"tool").unwrap();
        let files = vec![addons::InstalledFile {
            path: "tool".into(),
            size: 4,
            sha256: archive::sha256_file(&stage.join("tool")).unwrap(),
        }];
        staged.push((part(id, Sha256Digest::from_bytes([0; 32])), (stage, files)));
    }
    let (_, tools) = swap_in(&dir, &work, Record::default(), staged, |from, to| {
        std::fs::rename(from, to)
    })
    .unwrap();
    let outside: Vec<_> = std::fs::read_dir(root.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(outside, ["media"]);
    for id in ComponentId::ALL {
        assert!(dir.join(id.as_str()).join("tool").is_file(), "{id}");
    }
    assert!(tools.is_some());
}

#[test]
fn a_release_file_must_announce_a_size_within_the_cap() {
    assert!(fits(Some(100), 100).is_ok());
    assert!(fits(Some(101), 100).is_err());
    assert!(fits(None, 100).is_err());
}

#[test]
fn versions_read_plainly() {
    let parts = [
        (ComponentId::Deno, "2.9.7"),
        (ComponentId::Ffmpeg, "n9.0.2-25-g67b60c310b"),
        (ComponentId::YtDlp, "2026.08.19"),
    ];
    assert_eq!(
        describe(parts.into_iter()),
        "yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7"
    );
    assert_eq!(file_safe("v1/../2"), "v1_.._2");
}
