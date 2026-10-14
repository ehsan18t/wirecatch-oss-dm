//! Per-user local connections between the Wirecatch app and its clients (the
//! browser bridge and, later, the CLI).
//!
//! The app calls [`Listener::bind`] on [`endpoint_for`] its [`Channel`] and serves each accepted
//! [`Connection`]. A client calls [`connect`] on the same endpoint. Frames on a
//! connection use `wirecatch_protocol::frame` (4-byte little-endian length plus
//! JSON, capped at 1 MiB).
//!
//! # Security model
//!
//! The connection carries browser cookies, so both ends check who is on the other
//! side, and nothing is sent before the check passes:
//!
//! - **Windows**: the endpoint is the named pipe `\\.\pipe\wirecatch-app-<user SID>`
//!   (`wirecatch-app-dev-` for debug builds, from `config/product.toml`).
//!   The listener creates it with `FILE_FLAG_FIRST_PIPE_INSTANCE` (so it fails
//!   instead of joining a pipe someone else created first), rejects remote
//!   clients, and sets an explicit security descriptor whose owner is the current
//!   user and whose protected DACL has a single entry granting the current user
//!   full access. Nobody else, not even SYSTEM or Administrators, is granted
//!   anything (SYSTEM and administrators can take ownership of any object anyway,
//!   so listing them would add exposure without adding anything we need). A
//!   mandatory label at the listener's own integrity level keeps out programs
//!   running with fewer rights. The listener does not rely on the descriptor alone:
//!   it reads each client's token and refuses one that is another user, runs at a
//!   lower integrity level than the listener or in a sandbox (a restricted or
//!   `AppContainer` token), or hides who it is: a client must open the pipe at the
//!   identification level, as [`connect`] does, and one that opens it anonymously or
//!   without naming a level is refused. The connector
//!   reads the owner of the pipe it opened and refuses it unless it is the current
//!   user; it opens the pipe at the identification impersonation level, so the
//!   listener can tell who it is but a hostile server cannot act as the client.
//! - **Linux**: the endpoint is `$XDG_RUNTIME_DIR/wirecatch/app.sock` (`app-dev.sock`
//!   for debug builds), or
//!   `/tmp/wirecatch-<uid>/app.sock` when `XDG_RUNTIME_DIR` is unset. The folder is
//!   created with mode 0700 and refused if it already exists with any group or
//!   other access, is not owned by the current user, or is a symbolic link. The
//!   socket gets mode 0600. A lock file next to the socket makes sure only one
//!   listener owns the endpoint, and a stale socket is removed only while holding
//!   that lock and only when nothing answers on it. The listener refuses peers
//!   whose credentials (`SO_PEERCRED`) are another user, and the connector refuses
//!   a server whose credentials are another user.

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
    #[must_use]
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
    /// The endpoint name is held by a different user account, so the listener did
    /// not start (and nothing was exposed).
    #[error(
        "Another user account ({owner}) is holding the connection point {endpoint} that Wirecatch needs, so Wirecatch did not start listening and nothing was exposed. That account may be trying to intercept your downloads. Ask that user to sign out, or restart the computer, then open Wirecatch again."
    )]
    EndpointTaken {
        /// The endpoint that is taken.
        endpoint: String,
        /// Who holds it, as far as could be told.
        owner: String,
    },
    /// A client that is not the current user, or (Windows) runs with fewer rights
    /// than the listener or would not say who it is, tried to connect and was
    /// refused. The listener stays usable; keep calling [`Listener::accept`].
    #[error("Refused a connection from {peer}.")]
    PeerRefused {
        /// Who tried, as far as could be told: another user account with its uid
        /// (Linux) or SID (Windows), a program of the user's own at a lower integrity
        /// level, or a client that would not say who it is.
        peer: String,
    },
    /// The folder that holds the socket is not private to the current user.
    #[error("The folder {} is not safe to use: {reason}. {fix}", path.display())]
    UnsafeDirectory {
        /// The folder that was checked.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
        /// What the user can do about it.
        fix: String,
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

pub use wirecatch_product::Channel;

#[cfg(windows)]
pub use windows::{ProcessAccount, Sid};

/// Returns the endpoint of the release app for the current user: the one the
/// browser bridge and the CLI connect to. Same as
/// `endpoint_for(Channel::Release)`.
///
/// # Errors
///
/// See [`endpoint_for`].
pub fn endpoint() -> Result<Endpoint, IpcError> {
    endpoint_for(Channel::Release)
}

/// Returns the endpoint of `channel`'s app for the current user. Each channel has
/// its own (D13), so a debug build never serves or reaches an installed release.
///
/// On Linux this also creates the containing folder with mode 0700 if it is
/// missing, and refuses a folder that already exists but is not private to the
/// current user.
///
/// # Errors
///
/// [`IpcError::Io`] when the current user cannot be read (Windows) or the folder
/// cannot be created or inspected (Linux), and [`IpcError::UnsafeDirectory`] when the
/// folder exists but is not private to the current user.
pub fn endpoint_for(channel: Channel) -> Result<Endpoint, IpcError> {
    #[cfg(windows)]
    {
        windows::endpoint(channel)
    }
    #[cfg(unix)]
    {
        unix::endpoint(channel)
    }
}

/// How the current process holds administrator rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    /// No administrator rights: a standard account, or an administrator's normal start. Every process a Linux user starts.
    Standard,
    /// Started with "Run as administrator": a step the person took for this one program, while the browser and everything else run without those rights.
    Elevated,
    /// The account has administrator rights in every process, because user account control is turned off or it is the built-in Administrator. The browser runs the same way, so there is no normal start to ask for.
    AlwaysAdministrator,
}

/// How the current process holds administrator rights.
///
/// The app must not serve its endpoint from an [`Elevation::Elevated`] start: it would hold rights that the programs talking to it do not. The listener already keeps unelevated programs out of an elevated server's pipe; the app also refuses such a start, since the browser extension could not reach it either. On Linux an elevated process (root) is another user, whose socket the user's programs cannot open, so this is always [`Elevation::Standard`] there.
///
/// # Errors
///
/// Fails when Windows cannot report the process's token.
pub fn elevation() -> io::Result<Elevation> {
    #[cfg(windows)]
    {
        windows::elevation()
    }
    #[cfg(unix)]
    {
        Ok(Elevation::Standard)
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
    /// # Errors
    ///
    /// [`IpcError::AlreadyInUse`] when another listener of the current user owns the
    /// endpoint, [`IpcError::EndpointTaken`] when another user account holds it (Windows),
    /// [`IpcError::UnsafeDirectory`] when the socket's folder is not private (Linux), and
    /// [`IpcError::Io`] when the operating system refuses to create it.
    pub fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        #[cfg(windows)]
        let inner = windows::PipeListener::bind(endpoint)?;
        #[cfg(unix)]
        let inner = unix::SocketListener::bind(endpoint)?;
        Ok(Self { inner })
    }

    /// Waits for the next client. Cancelling the returned future is safe.
    ///
    /// # Errors
    ///
    /// [`IpcError::PeerRefused`] when a client was turned away for not being the
    /// current user (see the variant), and [`IpcError::Io`] when a client failed to
    /// connect or who it is could not be read. An error concerns
    /// only that one attempt: the listener stays usable, so keep calling `accept`.
    pub async fn accept(&mut self) -> Result<Connection, IpcError> {
        self.inner.accept().await
    }

    /// The endpoint this listener serves.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        self.inner.endpoint()
    }
}

/// Connects to the app at `endpoint`, after checking that the server belongs to
/// the current user. Must be called inside a tokio runtime with I/O and time
/// enabled.
///
/// # Errors
///
/// [`IpcError::NotRunning`] when nothing is listening, [`IpcError::WrongOwner`]
/// when the server belongs to someone else (nothing was sent to it), and
/// [`IpcError::Io`] when the connection cannot be opened or its owner read.
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
