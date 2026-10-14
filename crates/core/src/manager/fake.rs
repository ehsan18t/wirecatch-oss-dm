//! A stand-in for the engine in unit tests, so a test decides how a run behaves:
//! when the server "answers", and whether the run honours its stop request.

use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use wirecatch_engine::{CancellationToken, Error as EngineError, Outcome, PrivateNetwork};

use super::Manager;
use super::settle::Settle;

/// What a stand-in run returns.
pub(super) type EngineFuture = Pin<Box<dyn Future<Output = Result<Outcome, EngineError>> + Send>>;

/// One run as the stand-in sees it.
pub(super) struct FakeRun {
    /// Chooses the path from the server's answer; `None` once the path is settled.
    pub settle: Option<Settle>,
    pub cancel: CancellationToken,
    /// The rule for the local network the real engine would have been given.
    pub private_network: PrivateNetwork,
}

/// Plays the engine for every run of a manager.
pub(super) type FakeEngine = Arc<dyn Fn(FakeRun) -> EngineFuture + Send + Sync>;

/// Moves one file in place of the real move, so a test can make a move fail.
pub(super) type FakeMover = Arc<dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync>;

/// Starts `fake` on one run.
pub(super) fn run(
    fake: Option<FakeEngine>,
    settle: Option<Settle>,
    cancel: CancellationToken,
    private_network: PrivateNetwork,
) -> EngineFuture {
    let run = FakeRun {
        settle,
        cancel,
        private_network,
    };
    match fake {
        Some(fake) => fake(run),
        None => Box::pin(std::future::ready(Err(EngineError::Cancelled))),
    }
}

impl Manager {
    /// The stand-in engine, if a test set one.
    pub(super) fn fake_engine(&self) -> Option<FakeEngine> {
        slot(&self.inner.fake_engine).clone()
    }

    /// Runs every later download with `engine` instead of the real engine.
    pub(super) fn use_fake_engine(&self, engine: FakeEngine) {
        *slot(&self.inner.fake_engine) = Some(engine);
    }

    /// The stand-in file mover, if a test set one.
    pub(super) fn fake_mover(&self) -> Option<FakeMover> {
        slot(&self.inner.fake_mover).clone()
    }

    /// Moves every later download's files with `mover` instead of the file
    /// system's rename and copy.
    pub(super) fn use_fake_mover(&self, mover: FakeMover) {
        *slot(&self.inner.fake_mover) = Some(mover);
    }
}

/// Locks one of the stand-in slots. A lock poisoned by a panic in another holder
/// is taken over as it is, for the reason [`Manager::lock`] gives.
fn slot<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
