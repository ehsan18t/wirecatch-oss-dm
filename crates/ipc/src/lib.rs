//! Per-user local connections between the Wirecatch app and its clients (the
//! browser bridge and, later, the CLI).
//!
//! The app calls [`Listener::bind`] on [`endpoint()`] and serves each accepted
//! [`Connection`]. A client calls [`connect`] on the same endpoint. Frames on a
//! connection use `wirecatch_protocol::frame` (4-byte little-endian length plus
//! JSON, capped at 1 MiB).
//!
//! # Security model
//!
//! The connection carries browser cookies, so both ends check who is on the other
//! side, and nothing is sent before the check passes:
//!
//! - **Windows**: the endpoint is the named pipe `\\.\pipe\wirecatch-app-<user SID>`.
//!   The listener creates it with `FILE_FLAG_FIRST_PIPE_INSTANCE` (so it fails
//!   instead of joining a pipe someone else created first), rejects remote
//!   clients, and sets an explicit security descriptor whose owner is the current
//!   user and whose protected DACL has a single entry granting the current user
//!   full access. Nobody else, not even SYSTEM or Administrators, is granted
//!   anything (SYSTEM and administrators can take ownership of any object anyway,
//!   so listing them would add exposure without adding anything we need). The
//!   connector reads the owner of the pipe it opened and refuses it unless it is
//!   the current user; it also opens the pipe at the identification impersonation
//!   level so a hostile server cannot act as the client.
//! - **Linux**: the endpoint is `$XDG_RUNTIME_DIR/wirecatch/app.sock`, or
//!   `/tmp/wirecatch-<uid>/app.sock` when `XDG_RUNTIME_DIR` is unset. The folder is
//!   created with mode 0700 and refused if it already exists with any group or
//!   other access, is not owned by the current user, or is a symbolic link. The
//!   socket gets mode 0600. A lock file next to the socket makes sure only one
//!   listener owns the endpoint, and a stale socket is removed only while holding
//!   that lock and only when nothing answers on it. The listener refuses peers
//!   whose credentials (`SO_PEERCRED`) are another user, and the connector refuses
//!   a server whose credentials are another user.

#![warn(missing_docs)]

#[cfg(not(any(windows, unix)))]
compile_error!("wirecatch-ipc supports Windows and Linux only");

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

mod paths;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// The address the app listens on: a named pipe name on Windows, a socket file
/// path on Linux.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    name: OsString,
}

impl Endpoint {
    /// Wraps an explicit pipe name (Windows, for example `\\.\pipe\name`) or socket
    /// path (Linux).
    ///
    /// Production code uses [`endpoint()`]; this exists for tests and tools that
    /// need a private endpoint of their own. On Linux the folder that contains the
    /// socket must already exist, be owned by the current user and have no group
    /// or other access, or [`Listener::bind`] refuses it.
    pub fn from_raw(name: impl Into<OsString>) -> Self {
        Self { name: name.into() }
    }

    /// The pipe name or socket path.
    pub fn as_os_str(&self) -> &OsStr {
        &self.name
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name.to_string_lossy())
    }
}

/// Errors from the local connection layer. Messages are plain language and say
/// how to fix the problem where the user can.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IpcError {
    /// Nothing is listening at the endpoint, so the app is not running.
    #[error(
        "Wirecatch is not running (nothing is listening at {endpoint}). Open Wirecatch and try again."
    )]
    NotRunning {
        /// The endpoint that was tried.
        endpoint: String,
    },
    /// Another listener already owns the endpoint.
    #[error(
        "Another copy of Wirecatch is already listening at {endpoint}. Close it, or keep using that copy."
    )]
    AlreadyInUse {
        /// The endpoint that is taken.
        endpoint: String,
    },
    /// The server at the endpoint belongs to a different user account. Nothing was
    /// sent to it.
    #[error(
        "Refused to connect to {endpoint} because it belongs to another user account ({actual}), not to you ({expected}). Another user may be trying to intercept your downloads. Sign out the other users or restart the computer, then open Wirecatch again."
    )]
    WrongOwner {
        /// The endpoint that was tried.
        endpoint: String,
        /// The current user (a SID on Windows, a uid on Linux).
        expected: String,
        /// The owner that was found.
        actual: String,
    },
    /// A client from a different user account tried to connect and was refused.
    /// The listener stays usable; keep calling [`Listener::accept`].
    #[error("Refused a connection from another user account ({peer}).")]
    PeerRefused {
        /// The other user (a uid on Linux).
        peer: String,
    },
    /// The folder that holds the socket is not private to the current user.
    #[error(
        "The folder {} is not safe to use: {reason}. Delete the folder (or make it yours with `chmod 700`), then open Wirecatch again.",
        path.display()
    )]
    UnsafeDirectory {
        /// The folder that was checked.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// An operating system call failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
}

impl IpcError {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// Returns the endpoint of the app for the current user.
///
/// On Linux this also creates the containing folder with mode 0700 if it is
/// missing, and refuses a folder that already exists but is not private to the
/// current user.
pub fn endpoint() -> Result<Endpoint, IpcError> {
    #[cfg(windows)]
    {
        windows::endpoint()
    }
    #[cfg(unix)]
    {
        unix::endpoint()
    }
}

/// Listens for connections from the current user's clients.
///
/// Dropping the listener stops listening (on Linux it also removes the socket file
/// if it is still the one this listener created).
#[derive(Debug)]
pub struct Listener {
    #[cfg(windows)]
    inner: windows::PipeListener,
    #[cfg(unix)]
    inner: unix::SocketListener,
}

impl Listener {
    /// Starts listening at `endpoint`. Must be called inside a tokio runtime with
    /// I/O enabled.
    ///
    /// Fails with [`IpcError::AlreadyInUse`] if another listener owns the
    /// endpoint.
    pub fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        #[cfg(windows)]
        let inner = windows::PipeListener::bind(endpoint)?;
        #[cfg(unix)]
        let inner = unix::SocketListener::bind(endpoint)?;
        Ok(Self { inner })
    }

    /// Waits for the next client.
    ///
    /// An error concerns only that one attempt (for example
    /// [`IpcError::PeerRefused`]); the listener stays usable, so keep calling
    /// `accept`. Cancelling the returned future is safe.
    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        self.inner.accept().await
    }

    /// The endpoint this listener serves.
    pub fn endpoint(&self) -> &Endpoint {
        self.inner.endpoint()
    }
}

/// Connects to the app at `endpoint`, after checking that the server belongs to
/// the current user.
///
/// Returns [`IpcError::NotRunning`] when nothing is listening and
/// [`IpcError::WrongOwner`] when the server belongs to someone else. Must be
/// called inside a tokio runtime with I/O and time enabled.
pub async fn connect(endpoint: &Endpoint) -> Result<Connection, IpcError> {
    #[cfg(windows)]
    {
        windows::connect(endpoint, &windows::Sid::current_user()?).await
    }
    #[cfg(unix)]
    {
        unix::connect(endpoint, unix::current_uid()).await
    }
}

/// One open connection, from either side. Read and write frames with
/// `wirecatch_protocol::frame::{read_async, write_async}`, or split it with
/// `tokio::io::split`.
#[derive(Debug)]
pub struct Connection {
    inner: Inner,
}

#[derive(Debug)]
enum Inner {
    #[cfg(windows)]
    PipeServer(tokio::net::windows::named_pipe::NamedPipeServer),
    #[cfg(windows)]
    PipeClient(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

macro_rules! dispatch {
    ($self:ident, $s:ident => $e:expr) => {
        match &mut $self.get_mut().inner {
            #[cfg(windows)]
            Inner::PipeServer($s) => $e,
            #[cfg(windows)]
            Inner::PipeClient($s) => $e,
            #[cfg(unix)]
            Inner::Unix($s) => $e,
        }
    };
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        dispatch!(self, s => Pin::new(s).poll_read(cx, buf))
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        dispatch!(self, s => Pin::new(s).poll_write(cx, buf))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch!(self, s => Pin::new(s).poll_flush(cx))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        dispatch!(self, s => Pin::new(s).poll_shutdown(cx))
    }
}
