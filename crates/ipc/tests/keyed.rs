//! The keyed endpoint: only the listener writes the key, under a lock it holds while it
//! listens, and clients only read it (D41.4 to D41.6).
//!
//! Every test names its endpoint in folders of its own, with no runtime folder, so the Linux
//! socket is keyed in `/tmp` as in a session without one, and no test reaches the user's
//! running app or another test's endpoint.

use std::path::Path;

use wirecatch_ipc::{Channel, Folders, IpcError, Listener, endpoint_in};
use wirecatch_test_support::private_dir;

/// The key file's name, as the listener writes it.
const KEY_FILE: &str = "endpoint-key";

/// Folders that keep the key in `key` and name no runtime folder.
fn keyed_in(key: &Path) -> Folders {
    Folders {
        key: key.to_path_buf(),
        runtime: None,
    }
}

/// Removes the socket's folder in `/tmp`, which outlives its listener.
#[cfg(unix)]
fn remove_socket_folder(endpoint: &wirecatch_ipc::Endpoint) -> std::io::Result<()> {
    let folder = Path::new(endpoint.as_os_str())
        .parent()
        .ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::remove_dir_all(folder)
}

// A client that sees other folders than the app (a browser from an app store, or a snap or
// flatpak one) used to make a key of its own, which the app never read, so the two never
// met again (W4I review M2).
#[test]
fn a_client_never_creates_the_key() {
    let dir = private_dir("ipc-keyed-client").unwrap();
    let folder = dir.path().join("Wirecatch");
    match endpoint_in(Channel::Dev, &keyed_in(&folder)) {
        Err(IpcError::NoEndpointKey { path }) => assert_eq!(path, folder.join(KEY_FILE)),
        other => panic!("expected NoEndpointKey, got {other:?}"),
    }
    assert!(!folder.join(KEY_FILE).exists(), "a client wrote a key");
}

#[tokio::test]
async fn a_listener_writes_the_key_its_clients_read() {
    let dir = private_dir("ipc-keyed-listener").unwrap();
    let folders = keyed_in(&dir.path().join("Wirecatch"));
    let mut listener = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    let endpoint = endpoint_in(Channel::Dev, &folders).unwrap();
    assert_eq!(&endpoint, listener.endpoint());
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&endpoint), listener.accept());
    client.unwrap();
    server.unwrap();
    #[cfg(unix)]
    remove_socket_folder(&endpoint).unwrap();
}

// The lock, not the name, keeps a second copy out: a copy that cannot see the first one's
// pipe (one running with other rights) or that would write a new key still finds the lock
// taken (W4I review M1).
#[tokio::test]
async fn a_second_listener_is_refused_by_the_lock() {
    let dir = private_dir("ipc-keyed-second").unwrap();
    let folders = keyed_in(dir.path());
    let first = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    assert!(matches!(
        Listener::bind_keyed(Channel::Dev, &folders),
        Err(IpcError::AlreadyInUse { .. })
    ));
    let endpoint = endpoint_in(Channel::Dev, &folders).unwrap();
    assert_eq!(&endpoint, first.endpoint(), "the refused copy kept the key");
    #[cfg(unix)]
    remove_socket_folder(&endpoint).unwrap();
}

// The keyed tests all listened in the session's runtime folder on Linux, whatever key folder
// they named, so tests running at once took one socket and reset each other's connections, and
// a developer's tests reached their running Wirecatch Dev (W4 milestone review M1).
#[tokio::test]
async fn listeners_in_their_own_folders_never_meet() {
    let one = private_dir("ipc-keyed-one").unwrap();
    let two = private_dir("ipc-keyed-two").unwrap();
    let mut first = Listener::bind_keyed(Channel::Dev, &keyed_in(one.path())).unwrap();
    let mut second = Listener::bind_keyed(Channel::Dev, &keyed_in(two.path())).unwrap();
    let (at_first, at_second) = (first.endpoint().clone(), second.endpoint().clone());
    assert_ne!(at_first, at_second);
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&at_first), first.accept());
    client.unwrap();
    server.unwrap();
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&at_second), second.accept());
    client.unwrap();
    server.unwrap();
    #[cfg(unix)]
    for endpoint in [&at_first, &at_second] {
        remove_socket_folder(endpoint).unwrap();
    }
}

// The app logged its endpoint at every start, and the key in the name survives the log's
// redaction, which keeps long hex runs, so a log attached to a support request handed the key
// out (D41.4).
#[tokio::test]
async fn an_endpoint_never_prints_its_key() {
    let dir = private_dir("ipc-keyed-shown").unwrap();
    let folder = dir.path().join("Wirecatch");
    let listener = Listener::bind_keyed(Channel::Dev, &keyed_in(&folder)).unwrap();
    let key = std::fs::read_to_string(folder.join(KEY_FILE)).unwrap();
    let endpoint = listener.endpoint();
    let shown = format!(
        "{endpoint} {endpoint:?} {}",
        IpcError::NotRunning {
            endpoint: endpoint.to_string()
        }
    );
    assert!(!shown.contains(&key), "{shown}");
    assert!(
        endpoint.as_os_str().to_string_lossy().contains(&key),
        "the name itself keeps the key"
    );
    #[cfg(unix)]
    remove_socket_folder(endpoint).unwrap();
}

// Another account that once saw the keyed folder in `/tmp` (every account can list it) can
// create it while Wirecatch is closed; the listener then writes a new key instead of staying
// shut out (D41.6). A folder of the user's own that others may open stands in for one another
// account created, since both are refused the same way.
#[cfg(unix)]
#[tokio::test]
async fn a_listener_whose_keyed_folder_is_taken_listens_under_a_new_key() {
    use std::os::unix::fs::PermissionsExt;

    let dir = private_dir("ipc-keyed-taken").unwrap();
    let folders = keyed_in(dir.path());
    let first = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    let seen = first.endpoint().clone();
    drop(first);
    let taken = Path::new(seen.as_os_str()).parent().unwrap();
    std::fs::set_permissions(taken, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut listener = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    assert_ne!(listener.endpoint(), &seen);
    let current = endpoint_in(Channel::Dev, &folders).unwrap();
    assert_eq!(&current, listener.endpoint(), "clients read the new key");
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&current), listener.accept());
    client.unwrap();
    server.unwrap();
    remove_socket_folder(&seen).unwrap();
    remove_socket_folder(&current).unwrap();
}

// The runtime folder is the user's own, so a socket there takes no key and clients find it
// without reading one.
#[cfg(unix)]
#[tokio::test]
async fn a_socket_in_the_runtime_folder_takes_no_key() {
    let dir = private_dir("ipc-keyed-runtime").unwrap();
    let folders = Folders {
        key: dir.path().join("Wirecatch"),
        runtime: Some(dir.path().join("run")),
    };
    std::fs::create_dir(folders.runtime.as_ref().unwrap()).unwrap();
    let mut listener = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    let endpoint = endpoint_in(Channel::Dev, &folders).unwrap();
    assert_eq!(&endpoint, listener.endpoint());
    assert_eq!(
        Path::new(endpoint.as_os_str()),
        dir.path()
            .join("run")
            .join(wirecatch_product::IPC_RUNTIME_FOLDER)
            .join(Channel::Dev.socket_name())
    );
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&endpoint), listener.accept());
    client.unwrap();
    server.unwrap();
}

// An app from before the endpoint key, still running after an update, never writes one, so
// opening it is not enough: it has to be opened again.
#[test]
fn a_missing_key_asks_for_wirecatch_to_be_opened_again_after_an_update() {
    let missing = IpcError::NoEndpointKey {
        path: KEY_FILE.into(),
    }
    .to_string();
    assert!(
        missing.contains("since it was installed or updated"),
        "{missing}"
    );
    assert!(missing.contains("close it and open it again"), "{missing}");
}
