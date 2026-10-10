//! A download's name, category and folder settle on the server's answer on its
//! first run (B1), and the settled path is kept from then on.

use wirecatch_protocol::app::{Category, DownloadStatus, UiEvent};

use crate::common::{Config, Harness, PATIENCE, TestServer, test_data};

const KIB: usize = 1024;

async fn server_naming(name: &str, len: usize, bps: Option<u64>) -> TestServer {
    TestServer::start(
        test_data(len),
        Config {
            ranges: true,
            etag: Some("\"v1\"".into()),
            per_connection_bps: bps,
            content_disposition: Some(format!("attachment; filename=\"{name}\"")),
            ..Config::default()
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_servers_name_decides_name_category_and_folder() {
    let server = server_naming("Holiday Movie.mp4", 512 * KIB, Some(128 * 1024)).await;
    let mut h = Harness::new().await;
    let mut events = h.manager.subscribe();
    let row = h.add(&server.url("dl?id=1")).await;
    // Until the server answers, the name is a guess from the link.
    assert_eq!(
        (row.file_name.as_str(), row.category),
        ("dl", Category::Other)
    );

    let answered = h
        .wait_for(row.id, "the server's answer", |r| r.done > 0)
        .await;
    let expected = h.folders.downloads.join("Video").join("Holiday Movie.mp4");
    assert_eq!(std::path::Path::new(&answered.path), expected);
    assert_eq!(answered.file_name, "Holiday Movie.mp4");
    assert_eq!(answered.category, Category::Video);
    // The UI heard the real name.
    tokio::time::timeout(PATIENCE, async {
        loop {
            if let Ok(UiEvent::DownloadsChanged(rows)) = events.recv().await
                && rows
                    .iter()
                    .any(|r| r.id == row.id && r.file_name == "Holiday Movie.mp4")
            {
                return;
            }
        }
    })
    .await
    .expect("no event with the server's name");

    // The chosen path is kept: after a restart the download continues there.
    h.restart().await;
    assert_eq!(std::path::Path::new(&h.row(row.id).path), expected);
    server.configure(|c| c.per_connection_bps = None);
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::path::Path::new(&done.path), expected);
    assert_eq!(std::fs::read(&expected).unwrap(), server.data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_users_name_and_folder_win() {
    let server = server_naming("server.mp4", 4 * KIB, None).await;
    let h = Harness::new().await;
    let chosen = h.root.path().join("chosen");
    let row = h
        .add_with(&server.url("dl"), |r| {
            r.file_name = Some("mine.bin".into());
            r.folder = Some(chosen.to_string_lossy().into_owned());
        })
        .await;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::path::Path::new(&done.path), chosen.join("mine.bin"));
}

/// The capture toast's Change button is often pressed before the server has
/// answered; the folder chosen then must still be where the file ends up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_chosen_before_the_server_answers_is_kept() {
    let server = TestServer::start(
        test_data(4 * KIB),
        Config {
            ranges: true,
            etag: Some("\"v1\"".into()),
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            answer_delay: Some(std::time::Duration::from_millis(500)),
            ..Config::default()
        },
    )
    .await;
    let h = Harness::new().await;
    let row = h.add(&server.url("dl")).await;
    let chosen = h.root.path().join("chosen");
    h.manager
        .set_download_folder(row.id, &chosen.to_string_lossy())
        .await
        .unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::path::Path::new(&done.path), chosen.join("report.pdf"));
    assert_eq!(
        std::fs::read(chosen.join("report.pdf")).unwrap(),
        server.data
    );
}

/// The app stopped right after the server answered and the engine had started
/// the file, but before the chosen path reached the database. On the next run the
/// download must pick up its own part file, not start over as "name (1)" and
/// leave the old one behind.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_part_file_left_by_a_crash_is_picked_up_again() {
    let len = 512 * KIB;
    let server = server_naming("report.pdf", len, Some(128 * 1024)).await;
    let h = Harness::new().await;
    let documents = h.folders.downloads.join("Documents");
    let path = documents.join("report.pdf");

    // What the crashed run left: the part and resume files at the chosen path.
    let mut request =
        wirecatch_engine::DownloadRequest::new(server.url("dl").parse().unwrap(), &documents);
    request.exact_path = Some(path.clone());
    request.options.connections = 1;
    request.options.checkpoint_interval = std::time::Duration::from_millis(100);
    let progress = std::sync::Arc::new(wirecatch_engine::Progress::default());
    let cancel = wirecatch_engine::CancellationToken::new();
    let crashed = tokio::spawn(wirecatch_engine::download(
        request,
        progress.clone(),
        cancel.clone(),
    ));
    crate::common::wait_until("part of the file", || (progress.done() > 0).then_some(())).await;
    cancel.cancel();
    crashed.await.unwrap().unwrap();
    let [part, state] = crate::common::partial_files(&path.to_string_lossy());
    assert!(part.exists() && state.exists());

    server.configure(|c| c.per_connection_bps = None);
    let row = h.add(&server.url("dl")).await;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::path::Path::new(&done.path), path);
    assert_eq!(std::fs::read(&path).unwrap(), server.data);
    assert!(
        !part.exists() && !state.exists(),
        "partial files left behind"
    );
    let names: Vec<_> = std::fs::read_dir(&documents)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["report.pdf"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_server_name_twice_gets_a_free_name() {
    let server = server_naming("report.pdf", 4 * KIB, None).await;
    let h = Harness::new().await;
    let first = h.add(&server.url("a")).await.id;
    let second = h.add(&server.url("b")).await.id;
    let first = h.wait_status(first, DownloadStatus::Completed).await;
    let second = h.wait_status(second, DownloadStatus::Completed).await;
    let mut names = [first.file_name, second.file_name];
    names.sort();
    assert_eq!(
        names,
        ["report (1).pdf".to_string(), "report.pdf".to_string()]
    );
    assert_ne!(first.path, second.path);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_content_type_places_a_file_without_an_extension() {
    let server = TestServer::start(
        test_data(4 * KIB),
        Config {
            ranges: true,
            content_type: Some("audio/mpeg".into()),
            ..Config::default()
        },
    )
    .await;
    let h = Harness::new().await;
    let row = h.add(&server.url("stream")).await;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.category, Category::Audio);
    assert_eq!(
        std::path::Path::new(&done.path),
        h.folders.downloads.join("Music").join("stream")
    );
}
