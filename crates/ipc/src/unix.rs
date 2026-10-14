//! Linux transport: a mode 0600 Unix socket inside a mode 0700 folder, with peer
//! credential checks on both ends.

use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};
use wirecatch_product::Channel;

use crate::key::EndpointKey;
use crate::paths::{self, DirFacts};
use crate::{Connection, Endpoint, Inner, IpcError};

/// The effective user id of this process.
pub(crate) fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Whether the endpoint's name needs a key: only in the shared `/tmp` fallback, since the
/// runtime folder is the user's own.
pub(crate) fn needs_key() -> bool {
    paths::usable_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR").as_deref()).is_none()
}

/// `channel`'s socket for the current user: in the runtime folder, which is the user's own,
/// or else in the shared `/tmp`, in a folder named with the endpoint key that `key` gives,
/// since without one another account could create the folder first. `key` is called only
/// for `/tmp`.
pub(crate) fn endpoint(
    channel: Channel,
    key: impl FnOnce() -> Result<EndpointKey, IpcError>,
) -> Result<Endpoint, IpcError> {
    let uid = current_uid();
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR");
    let endpoint = if let Some(runtime) = paths::usable_runtime_dir(runtime_dir.as_deref()) {
        Endpoint::from_raw(paths::runtime_socket_path(channel, runtime))
    } else {
        let key = key()?;
        Endpoint::keyed(paths::shared_socket_path(channel, uid, key.as_str()), &key)
    };
    ensure_private_dir(parent_of(Path::new(endpoint.as_os_str()))?, uid)
        .map_err(|e| hiding_key(e, &endpoint))?;
    Ok(endpoint)
}

/// `error` with `endpoint`'s key hidden wherever it names the socket's folder or a file in
/// it, which in `/tmp` carry the key.
fn hiding_key(error: IpcError, endpoint: &Endpoint) -> IpcError {
    let hide = |text: &str| endpoint.hide_key_in(text).into_owned();
    match error {
        IpcError::UnsafeDirectory { path, reason, fix } => IpcError::UnsafeDirectory {
            path: hide(&path.to_string_lossy()).into(),
            reason: hide(&reason),
            fix: hide(&fix),
        },
        IpcError::Io { context, source } => IpcError::Io {
            context: hide(&context),
            source,
        },
        other => other,
    }
}

/// The base of the user's local data folders: `$XDG_DATA_HOME`, or `~/.local/share`.
pub(crate) fn local_data_base() -> Option<PathBuf> {
    paths::linux_data_home(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

fn parent_of(path: &Path) -> Result<&Path, IpcError> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            IpcError::io(
                format!(
                    "The socket path {} has no containing folder",
                    path.display()
                ),
                io::Error::from(io::ErrorKind::InvalidInput),
            )
        })
}

/// Creates `dir` with mode 0700 if it is missing, then checks that it is private
/// to `uid`. An existing folder is never loosened or tightened, only checked.
pub(crate) fn ensure_private_dir(dir: &Path, uid: u32) -> Result<(), IpcError> {
    match DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {
            // The umask can only remove bits, but set the exact mode anyway so the
            // result never depends on it.
            fs::set_permissions(dir, Permissions::from_mode(0o700)).map_err(|e| {
                IpcError::io(format!("Could not make {} private", dir.display()), e)
            })?;
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(IpcError::io(
                format!("Could not create the folder {}", dir.display()),
                e,
            ));
        }
    }
    verify_private_dir(dir, uid)
}

/// Checks, without following symbolic links, that `dir` is a folder owned by
/// `uid` with no group or other access.
pub(crate) fn verify_private_dir(dir: &Path, uid: u32) -> Result<(), IpcError> {
    let meta = fs::symlink_metadata(dir)
        .map_err(|e| IpcError::io(format!("Could not inspect the folder {}", dir.display()), e))?;
    let facts = DirFacts {
        is_dir: meta.file_type().is_dir(),
        is_symlink: meta.file_type().is_symlink(),
        owner_uid: meta.uid(),
        mode: meta.mode(),
    };
    paths::check_private_dir(facts, uid, &dir.display().to_string()).map_err(|problem| {
        IpcError::UnsafeDirectory {
            path: dir.to_path_buf(),
            reason: problem.reason,
            fix: problem.fix,
        }
    })
}

/// The server side.
#[derive(Debug)]
pub(crate) struct SocketListener {
    endpoint: Endpoint,
    listener: UnixListener,
    uid: u32,
    /// Device and inode of the socket file this listener created, so Drop never
    /// removes a file that a later listener put in its place.
    socket_id: (u64, u64),
    /// Held (with `flock`) for as long as the listener lives.
    _lock: File,
}

impl SocketListener {
    pub(crate) fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        Self::bind_named(endpoint).map_err(|e| hiding_key(e, endpoint))
    }

    fn bind_named(endpoint: &Endpoint) -> Result<Self, IpcError> {
        let uid = current_uid();
        let path = Path::new(endpoint.as_os_str());
        let dir = parent_of(path)?;
        verify_private_dir(dir, uid)?;
        let lock = acquire_lock(&paths::lock_path(path), endpoint)?;
        remove_stale_socket(path, endpoint)?;

        let listener = UnixListener::bind(path).map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse {
                IpcError::AlreadyInUse {
                    endpoint: endpoint.to_string(),
                }
            } else {
                IpcError::io(format!("Could not listen at {endpoint}"), e)
            }
        })?;
        // The folder is 0700, so nobody else could reach the socket in the moment
        // between bind and this call.
        fs::set_permissions(path, Permissions::from_mode(0o600))
            .map_err(|e| IpcError::io(format!("Could not make {endpoint} private"), e))?;
        let meta = fs::symlink_metadata(path)
            .map_err(|e| IpcError::io(format!("Could not inspect {endpoint}"), e))?;

        Ok(Self {
            endpoint: endpoint.clone(),
            listener,
            uid,
            socket_id: (meta.dev(), meta.ino()),
            _lock: lock,
        })
    }

    pub(crate) async fn accept(&mut self) -> Result<Connection, IpcError> {
        let (stream, _) = self
            .listener
            .accept()
            .await
            .map_err(|e| IpcError::io("Could not accept a connection", e))?;
        let peer = stream
            .peer_cred()
            .map_err(|e| IpcError::io("Could not check who is connecting", e))?
            .uid();
        // The socket mode already keeps other users out; this is a second check
        // that does not depend on file permissions.
        if peer != self.uid {
            return Err(IpcError::PeerRefused {
                peer: format!("another user account (uid {peer})"),
            });
        }
        Ok(Connection {
            inner: Inner::Unix(stream),
        })
    }

    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        let path = Path::new(self.endpoint.as_os_str());
        if let Ok(meta) = fs::symlink_metadata(path)
            && (meta.dev(), meta.ino()) == self.socket_id
        {
            leftover_is_stale_to_the_next_bind(&fs::remove_file(path));
        }
    }
}

/// Takes the outcome of removing the socket file on drop. A file left behind is handled as
/// stale by the next bind, so a failure cannot matter.
fn leftover_is_stale_to_the_next_bind(removed: &std::io::Result<()>) {
    let (Ok(()) | Err(_)) = removed;
}

/// Takes the exclusive lock that marks the live listener.
fn acquire_lock(path: &Path, endpoint: &Endpoint) -> Result<File, IpcError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| {
            IpcError::io(
                format!("Could not open the lock file {}", path.display()),
                e,
            )
        })?;
    // SAFETY: flock on a descriptor owned by `file`, which outlives the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = io::Error::last_os_error();
        return Err(if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            IpcError::AlreadyInUse {
                endpoint: endpoint.to_string(),
            }
        } else {
            IpcError::io(format!("Could not lock {}", path.display()), e)
        });
    }
    Ok(file)
}

/// Removes a socket file left behind by a listener that is gone. Called with the
/// lock held, so no Wirecatch listener can be alive; it still checks that nothing
/// answers before removing anything.
fn remove_stale_socket(path: &Path, endpoint: &Endpoint) -> Result<(), IpcError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(IpcError::io(format!("Could not inspect {endpoint}"), e)),
    };
    if !meta.file_type().is_socket() {
        return Err(IpcError::io(
            format!("{endpoint} exists and is not a socket. Remove it, then open Wirecatch again"),
            io::Error::from(io::ErrorKind::AlreadyExists),
        ));
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Err(IpcError::AlreadyInUse {
            endpoint: endpoint.to_string(),
        }),
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => fs::remove_file(path)
            .map_err(|e| IpcError::io(format!("Could not remove the old socket {endpoint}"), e)),
        Err(e) => Err(IpcError::io(
            format!("Could not check the old socket {endpoint}"),
            e,
        )),
    }
}

/// Connects and refuses the server unless its credentials are `expected_uid`.
/// Nothing is written before the check passes.
pub(crate) async fn connect(
    endpoint: &Endpoint,
    expected_uid: u32,
) -> Result<Connection, IpcError> {
    let stream = match UnixStream::connect(endpoint.as_os_str()).await {
        Ok(stream) => stream,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Err(IpcError::NotRunning {
                endpoint: endpoint.to_string(),
            });
        }
        Err(e) => {
            return Err(IpcError::io(
                format!("Could not open the connection at {endpoint}"),
                e,
            ));
        }
    };
    let peer = stream
        .peer_cred()
        .map_err(|e| IpcError::io("Could not check who owns the connection", e))?
        .uid();
    verify_owner(endpoint, expected_uid, peer)?;
    Ok(Connection {
        inner: Inner::Unix(stream),
    })
}

/// Refuses unless `actual` is `expected`.
pub(crate) fn verify_owner(
    endpoint: &Endpoint,
    expected: u32,
    actual: u32,
) -> Result<(), IpcError> {
    if expected == actual {
        Ok(())
    } else {
        Err(IpcError::WrongOwner {
            endpoint: endpoint.to_string(),
            expected: format!("uid {expected}"),
            actual: format!("uid {actual}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::{PrivateDir, private_dir};

    /// A private (0700) folder for one test, removed on drop.
    fn new_dir() -> PrivateDir {
        private_dir("ipc").unwrap()
    }

    /// The app's endpoint inside `dir`.
    fn endpoint_in(dir: &PrivateDir) -> Endpoint {
        Endpoint::from_raw(dir.path().join(Channel::Release.socket_name()))
    }

    // The app's log keeps long hexadecimal runs (D40.10), so an error naming the `/tmp`
    // folder handed the endpoint key to anyone the log was sent to (D41.4).
    #[test]
    fn an_error_about_the_keyed_folder_hides_the_key() {
        const KEY: &str = "0123456789abcdef0123456789abcdef";
        let key = EndpointKey::parse(KEY).unwrap();
        let folder = format!("/tmp/wirecatch-1000-{KEY}");
        let endpoint = Endpoint::keyed(format!("{folder}/app.sock"), &key);
        let errors = [
            IpcError::UnsafeDirectory {
                path: folder.clone().into(),
                reason: format!("{folder} is not yours"),
                fix: format!("Remove {folder}"),
            },
            IpcError::io(
                format!("Could not open the lock file {folder}/app.sock.lock"),
                io::Error::from(io::ErrorKind::PermissionDenied),
            ),
        ];
        for error in errors {
            let shown = hiding_key(error, &endpoint);
            let written = format!("{shown} {shown:?}");
            assert!(!written.contains(KEY), "{written}");
            assert!(written.contains("/tmp/wirecatch-1000-***"), "{written}");
        }
    }

    #[tokio::test]
    async fn socket_is_0600_inside_a_0700_folder() {
        let dir = new_dir();
        let endpoint = endpoint_in(&dir);
        let _listener = SocketListener::bind(&endpoint).unwrap();
        let socket = fs::symlink_metadata(endpoint.as_os_str()).unwrap();
        assert!(socket.file_type().is_socket());
        assert_eq!(socket.mode() & 0o777, 0o600);
        assert_eq!(socket.uid(), current_uid());
        let folder = fs::symlink_metadata(dir.path()).unwrap();
        assert_eq!(folder.mode() & 0o777, 0o700);
    }

    #[test]
    fn a_new_folder_is_created_private() {
        let parent = new_dir();
        let dir = parent.path().join("wirecatch");
        ensure_private_dir(&dir, current_uid()).unwrap();
        let meta = fs::symlink_metadata(&dir).unwrap();
        assert!(meta.is_dir());
        assert_eq!(meta.mode() & 0o777, 0o700);
        // Running it again on the now-existing folder accepts it.
        ensure_private_dir(&dir, current_uid()).unwrap();
    }

    #[test]
    fn an_existing_open_folder_is_refused_and_left_alone() {
        let parent = new_dir();
        let dir = parent.path().join("open");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            ensure_private_dir(&dir, current_uid()),
            Err(IpcError::UnsafeDirectory { .. })
        ));
        assert_eq!(fs::symlink_metadata(&dir).unwrap().mode() & 0o777, 0o755);
    }

    #[test]
    fn a_folder_owned_by_another_uid_is_refused() {
        let parent = new_dir();
        // Stand in for another user by expecting a different uid.
        let other = current_uid().wrapping_add(1);
        assert!(matches!(
            verify_private_dir(parent.path(), other),
            Err(IpcError::UnsafeDirectory { .. })
        ));
    }

    #[test]
    fn a_symlinked_folder_is_refused() {
        let parent = new_dir();
        let real = parent.path().join("real");
        DirBuilder::new().mode(0o700).create(&real).unwrap();
        let link = parent.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(matches!(
            ensure_private_dir(&link, current_uid()),
            Err(IpcError::UnsafeDirectory { .. })
        ));
    }

    #[tokio::test]
    async fn bind_refuses_an_open_folder() {
        let parent = new_dir();
        let dir = parent.path().join("open");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        let endpoint = Endpoint::from_raw(dir.join(Channel::Release.socket_name()));
        assert!(matches!(
            SocketListener::bind(&endpoint),
            Err(IpcError::UnsafeDirectory { .. })
        ));
    }

    #[tokio::test]
    async fn connector_refuses_a_server_owned_by_another_uid() {
        let dir = new_dir();
        let endpoint = endpoint_in(&dir);
        let _listener = SocketListener::bind(&endpoint).unwrap();
        let other = current_uid().wrapping_add(1);
        match connect(&endpoint, other).await {
            Err(IpcError::WrongOwner {
                expected, actual, ..
            }) => {
                assert_eq!(expected, format!("uid {other}"));
                assert_eq!(actual, format!("uid {}", current_uid()));
            }
            other => panic!("expected WrongOwner, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_stale_socket_is_replaced() {
        let dir = new_dir();
        let endpoint = endpoint_in(&dir);
        drop(std::os::unix::net::UnixListener::bind(endpoint.as_os_str()).unwrap());
        assert!(fs::symlink_metadata(endpoint.as_os_str()).is_ok());
        let _listener = SocketListener::bind(&endpoint).unwrap();
        connect(&endpoint, current_uid()).await.unwrap();
    }

    #[tokio::test]
    async fn a_live_listener_is_not_replaced() {
        let dir = new_dir();
        let endpoint = endpoint_in(&dir);
        let _first = SocketListener::bind(&endpoint).unwrap();
        assert!(matches!(
            SocketListener::bind(&endpoint),
            Err(IpcError::AlreadyInUse { .. })
        ));
    }

    /// Release and debug builds listen on sibling sockets in one folder (D13), so each
    /// socket's listener holds a lock of its own.
    #[tokio::test]
    async fn listeners_on_sibling_sockets_in_one_folder_both_bind() {
        let dir = new_dir();
        let release = Endpoint::from_raw(dir.path().join("app.sock"));
        let dev = Endpoint::from_raw(dir.path().join("app-dev.sock"));
        let _release = SocketListener::bind(&release).unwrap();
        let _dev = SocketListener::bind(&dev).unwrap();
        connect(&release, current_uid()).await.unwrap();
        connect(&dev, current_uid()).await.unwrap();
    }

    #[tokio::test]
    async fn dropping_the_listener_removes_its_socket() {
        let dir = new_dir();
        let endpoint = endpoint_in(&dir);
        drop(SocketListener::bind(&endpoint).unwrap());
        assert!(fs::symlink_metadata(endpoint.as_os_str()).is_err());
        assert!(matches!(
            connect(&endpoint, current_uid()).await,
            Err(IpcError::NotRunning { .. })
        ));
    }
}
