//! The keyed endpoint: only the listener writes the key, under a lock it holds while it
//! listens, and clients only read it (D41.4 to D41.6).

use wirecatch_ipc::{Channel, IpcError, endpoint_keyed_in};
use wirecatch_test_support::private_dir;

/// The key file's name, as the listener writes it.
const KEY_FILE: &str = "endpoint-key";

// A client that sees other folders than the app (a browser from an app store, or a snap or
// flatpak one) used to make a key of its own, which the app never read, so the two never
// met again (W4I review M2).
#[test]
fn a_client_never_creates_the_key() {
    let dir = private_dir("ipc-keyed-client").unwrap();
    let folder = dir.path().join("Wirecatch");
    match endpoint_keyed_in(Channel::Dev, &folder) {
        Err(IpcError::NoEndpointKey { path }) => assert_eq!(path, folder.join(KEY_FILE)),
        #[cfg(unix)]
        // A runtime folder needs no key, so the client does not look for one.
        Ok(_) if std::env::var_os("XDG_RUNTIME_DIR").is_some() => {}
        other => panic!("expected NoEndpointKey, got {other:?}"),
    }
    assert!(!folder.join(KEY_FILE).exists(), "a client wrote a key");
}

#[tokio::test]
async fn a_listener_writes_the_key_its_clients_read() {
    let dir = private_dir("ipc-keyed-listener").unwrap();
    let folder = dir.path().join("Wirecatch");
    let mut listener = wirecatch_ipc::Listener::bind_keyed(Channel::Dev, &folder).unwrap();
    let endpoint = endpoint_keyed_in(Channel::Dev, &folder).unwrap();
    assert_eq!(&endpoint, listener.endpoint());
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&endpoint), listener.accept());
    client.unwrap();
    server.unwrap();
}

// The lock, not the name, keeps a second copy out: a copy that cannot see the first one's
// pipe (one running with other rights) or that would write a new key still finds the lock
// taken (W4I review M1).
#[tokio::test]
async fn a_second_listener_is_refused_by_the_lock() {
    let dir = private_dir("ipc-keyed-second").unwrap();
    let _first = wirecatch_ipc::Listener::bind_keyed(Channel::Dev, dir.path()).unwrap();
    assert!(matches!(
        wirecatch_ipc::Listener::bind_keyed(Channel::Dev, dir.path()),
        Err(IpcError::AlreadyInUse { .. })
    ));
}

// The app logged its endpoint at every start, and the key in the name survives the log's
// redaction, which keeps long hex runs, so a log attached to a support request handed the key
// out (D41.4).
#[tokio::test]
async fn an_endpoint_never_prints_its_key() {
    let dir = private_dir("ipc-keyed-shown").unwrap();
    let folder = dir.path().join("Wirecatch");
    let listener = wirecatch_ipc::Listener::bind_keyed(Channel::Dev, &folder).unwrap();
    let key = std::fs::read_to_string(folder.join(KEY_FILE)).unwrap();
    let endpoint = listener.endpoint();
    let shown = format!(
        "{endpoint} {endpoint:?} {}",
        IpcError::NotRunning {
            endpoint: endpoint.to_string()
        }
    );
    assert!(!shown.contains(&key), "{shown}");
    #[cfg(unix)]
    if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        return;
    }
    assert!(
        endpoint.as_os_str().to_string_lossy().contains(&key),
        "the name itself keeps the key"
    );
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
