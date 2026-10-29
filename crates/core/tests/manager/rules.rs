//! Auto queues rule new downloads, and a download keeps why it is in its queue (D72.3, D72.8).

use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AddRequest, QueueSettings};
use wirecatch_protocol::rules::{
    AddDownloads, DraftRules, LinkTrial, NameSyntax, QueueReason, QueueRules, Rule,
};

use crate::common::{Harness, TestResult, fast_server};

fn auto_queue(h: &Harness, name: &str, rules: QueueRules) -> TestResult<QueueId> {
    let settings = QueueSettings {
        add_downloads: AddDownloads::Auto(QueueRules {
            enabled: true,
            ..rules
        }),
        ..QueueSettings::new(name)
    };
    Ok(h.manager.create_queue(settings)?.id)
}

fn paused(r: &mut AddRequest) {
    r.start = false;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_matched_download_joins_its_auto_queue_and_keeps_why() -> TestResult {
    let mut h = Harness::new().await?;
    let server = fast_server(1024).await?;
    let local = auto_queue(
        &h,
        "This machine",
        QueueRules {
            sites: vec!["127.0.0.1".into()],
            ..QueueRules::default()
        },
    )?;
    let matched = QueueReason::Matched(Rule::Site("127.0.0.1".into()));

    let row = h.add_with(server.url("/a.bin"), paused).await?;
    assert_eq!((row.queue, &row.queue_reason), (local, &matched));
    h.restart().await?;
    assert_eq!(h.row(row.id).queue_reason, matched);

    h.manager.move_to_queue(&[row.id], DEFAULT_QUEUE_ID)?;
    assert_eq!(
        (h.row(row.id).queue, h.row(row.id).queue_reason),
        (DEFAULT_QUEUE_ID, QueueReason::Picked)
    );

    let picked = h
        .add_with(server.url("/b.bin"), |r| {
            r.start = false;
            r.queue = Some(DEFAULT_QUEUE_ID);
        })
        .await?;
    assert_eq!(
        (picked.queue, picked.queue_reason),
        (DEFAULT_QUEUE_ID, QueueReason::Picked)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_no_queue_rules_joins_the_default_queue() -> TestResult {
    let h = Harness::new().await?;
    let server = fast_server(1024).await?;
    let reports = auto_queue(
        &h,
        "Reports",
        QueueRules {
            file_names: vec!["report-*.pdf".into()],
            ..QueueRules::default()
        },
    )?;

    let named = h
        .add_with(server.url("/x"), |r| {
            r.start = false;
            r.file_name = Some("Report-2026.pdf".into());
        })
        .await?;
    assert_eq!(
        (named.queue, named.queue_reason),
        (
            reports,
            QueueReason::Matched(Rule::FileName("report-*.pdf".into()))
        )
    );

    let other = h.add_with(server.url("/notes.txt"), paused).await?;
    assert_eq!(
        (other.queue, other.queue_reason),
        (DEFAULT_QUEUE_ID, QueueReason::DefaultQueue)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sidebar_order_settles_a_tie_and_keeps_the_default_queue_first() -> TestResult {
    let mut h = Harness::new().await?;
    let server = fast_server(1024).await?;
    let same = || QueueRules {
        file_types: vec!["bin".into()],
        ..QueueRules::default()
    };
    let first = auto_queue(&h, "First", same())?;
    let second = auto_queue(&h, "Second", same())?;
    let matched_by = |h: &Harness, row: &wirecatch_protocol::app::DownloadSummary| {
        assert_eq!(
            row.queue_reason,
            QueueReason::Matched(Rule::FileType("bin".into()))
        );
        h.row(row.id).queue
    };
    let row = h.add_with(server.url("/a.bin"), paused).await?;
    assert_eq!(matched_by(&h, &row), first);

    h.manager
        .set_queue_order(&[DEFAULT_QUEUE_ID, second, first])?;
    h.restart().await?;
    let order: Vec<QueueId> = h.manager.list_queues().iter().map(|q| q.id).collect();
    assert_eq!(order, [DEFAULT_QUEUE_ID, second, first]);
    let row = h.add_with(server.url("/b.bin"), paused).await?;
    assert_eq!(matched_by(&h, &row), second);

    for refused in [
        vec![second, DEFAULT_QUEUE_ID, first],
        vec![DEFAULT_QUEUE_ID, second],
        vec![DEFAULT_QUEUE_ID, second, second, first],
    ] {
        assert!(h.manager.set_queue_order(&refused).is_err(), "{refused:?}");
    }
    let unchanged: Vec<QueueId> = h.manager.list_queues().iter().map(|q| q.id).collect();
    assert_eq!(unchanged, order);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trying_a_link_checks_the_drafted_rules_as_saving_does() -> TestResult {
    let h = Harness::new().await?;
    let trial = |rules: QueueRules| LinkTrial {
        url: "https://files.example.com/a-final.zip".into(),
        draft: Some(DraftRules {
            queue: None,
            add_downloads: AddDownloads::Auto(QueueRules {
                enabled: true,
                ..rules
            }),
        }),
    };

    let bad = h.manager.try_link(&trial(QueueRules {
        file_names: vec!["*-final.*".into()],
        file_name_syntax: NameSyntax::Regex,
        ..QueueRules::default()
    }));
    let message = bad.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains("not a regular expression"), "{message}");

    let found = h.manager.try_link(&trial(QueueRules {
        sites: vec!["  example.com ".into()],
        ..QueueRules::default()
    }))?;
    assert_eq!(
        found.matched.map(|t| t.rule),
        Some(Rule::Site("example.com".into()))
    );
    Ok(())
}
