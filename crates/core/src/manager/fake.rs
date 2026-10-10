//! A stand-in for the engine in unit tests, so a test decides how a run behaves:
//! when the server "answers", and whether the run honours its stop request.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use wirecatch_engine::{CancellationToken, Error as EngineError, Outcome};

use super::Manager;
use super::settle::Settle;

/// What a stand-in run returns.
pub(super) type EngineFuture = Pin<Box<dyn Future<Output = Result<Outcome, EngineError>> + Send>>;

/// One run as the stand-in sees it.
pub(super) struct FakeRun {
    /// Chooses the path from the server's answer; `None` once the path is settled.
    pub settle: Option<Settle>,
    /// The download's path when the run started.
    pub path: PathBuf,
    pub cancel: CancellationToken,
}

/// Plays the engine for every run of a manager.
pub(super) type FakeEngine = Arc<dyn Fn(FakeRun) -> EngineFuture + Send + Sync>;

/// Starts `fake` on one run.
pub(super) fn run(
    fake: Option<FakeEngine>,
    settle: Option<Settle>,
    path: PathBuf,
    cancel: CancellationToken,
) -> EngineFuture {
    let run = FakeRun {
        settle,
        path,
        cancel,
    };
    match fake {
        Some(fake) => fake(run),
        None => Box::pin(std::future::ready(Err(EngineError::Cancelled))),
    }
}

impl Manager {
    /// The stand-in engine, if a test set one.
    pub(super) fn fake_engine(&self) -> Option<FakeEngine> {
        self.inner.fake_engine.lock().ok()?.clone()
    }

    /// Runs every later download with `engine` instead of the real engine.
    pub(super) fn use_fake_engine(&self, engine: FakeEngine) {
        if let Ok(mut fake) = self.inner.fake_engine.lock() {
            *fake = Some(engine);
        }
    }
}
