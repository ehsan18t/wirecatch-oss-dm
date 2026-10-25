//! Every sample of what the store saves still loads with this build (D81.3).

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use wirecatch_protocol::app::{AppSettings, QueueSettings, UserError};
use wirecatch_records::batch::StoredBatch;
use wirecatch_records::media::output::{Container, Extras, OutputJob};
use wirecatch_records::media::{FormatId, MediaJob, StoredMedia};
use wirecatch_records::origin::Origin;
use wirecatch_records::proxy::Logins;
use wirecatch_store::LastIds;

type Check = fn(&str) -> Result<(), String>;

/// Each folder of `samples` and the check that reads its files as the type the store saves there.
const KINDS: &[(&str, Check)] = &[
    ("settings", loads::<AppSettings>),
    ("queue_settings", loads::<QueueSettings>),
    ("last_ids", loads::<LastIds>),
    ("batches", loads::<Vec<StoredBatch>>),
    ("proxy_logins", loads::<Logins>),
    ("origin", loads::<Origin>),
    ("download_error", loads::<UserError>),
    ("media", loads::<StoredMedia>),
];

fn loads<T: DeserializeOwned>(json: &str) -> Result<(), String> {
    serde_json::from_str::<T>(json)
        .map(drop)
        .map_err(|e| e.to_string())
}

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("samples")
}

fn job(path: &str) -> Option<MediaJob> {
    let json = fs::read_to_string(samples().join(path)).ok()?;
    serde_json::from_str::<StoredMedia>(&json).ok()?.job
}

#[test]
fn every_saved_sample_loads() {
    let mut failures = Vec::new();
    for folder in fs::read_dir(samples()).unwrap() {
        let folder = folder.unwrap().path();
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        let Some((_, check)) = KINDS.iter().find(|(kind, _)| *kind == name) else {
            failures.push(format!(
                "{name}: no type is listed for this folder in KINDS"
            ));
            continue;
        };
        let files: Vec<PathBuf> = fs::read_dir(&folder)
            .unwrap()
            .map(|file| file.unwrap().path())
            .collect();
        if files.is_empty() {
            failures.push(format!("{name}: no samples"));
        }
        for file in files {
            if let Err(e) = check(&fs::read_to_string(&file).unwrap()) {
                failures.push(format!("{}: {e}", file.display()));
            }
        }
    }
    for (kind, _) in KINDS {
        if !samples().join(kind).is_dir() {
            failures.push(format!("{kind}: no samples folder"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_page_saved_with_its_selection_reads_from_its_ids() {
    let id = |id: &str| FormatId::try_from(id.to_string()).unwrap();
    assert_eq!(
        job("media/page_with_format_field.json"),
        Some(MediaJob::Page {
            video: Some(id("137")),
            audio: Some(id("140")),
            output: None,
            keep_ext: None,
        })
    );
}

fn output(path: &str) -> Option<OutputJob> {
    match job(path)? {
        MediaJob::Page {
            output: Some(output),
            ..
        } => Some(*output),
        _ => None,
    }
}

#[test]
fn a_conversion_saved_before_extras_keeps_its_chapters_and_tags() {
    let stored = output("media/page_before_extras.json").unwrap();
    assert_eq!(
        stored.extras,
        Extras {
            chapters: true,
            tags: true,
        }
    );
    assert_eq!(stored.container, Container::Mp4);
    assert_eq!(stored.duration_ms, Some(1000));
}

#[test]
fn a_conversion_saved_without_its_optional_fields_adds_none() {
    let older = output("media/page_before_subtitle_files.json").unwrap();
    assert_eq!(older.extras, Extras::default());
    assert!(!older.subtitle_files && !older.original);
}
