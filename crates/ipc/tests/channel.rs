//! The endpoint a channel's app listens at and the one its clients look for, both found from
//! the channel alone, as the app and the bridge find them (IPB-10, D41.4).
//!
//! This binary runs this one test, since it points the user's local data folder at a folder
//! of its own through the environment.

use wirecatch_ipc::{Channel, Folders, IpcError, Listener, endpoint_for};
use wirecatch_test_support::private_dir;

// The app listened at a name worked out from the user's SID (Windows) or uid (the `/tmp`
// fallback on Linux) alone, so another account could take it first and keep Wirecatch from
// starting (IPB-10).
#[tokio::test(flavor = "current_thread")]
async fn a_channels_endpoint_carries_the_key_its_listener_wrote() {
    let dir = private_dir("ipc-channel").unwrap();
    // SAFETY: this test binary runs this one test, on one thread, so nothing else reads the
    // environment meanwhile.
    unsafe {
        #[cfg(windows)]
        std::env::set_var("LOCALAPPDATA", dir.path());
        #[cfg(unix)]
        {
            std::env::set_var("XDG_DATA_HOME", dir.path());
            // The shared `/tmp` fallback, where the key matters.
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
    }
    let folders = Folders::of_user(Channel::Dev).unwrap();
    let folder = &folders.key;
    assert_eq!(folder, &dir.path().join(Channel::Dev.data_folder()));

    // Before the app first listens, a client finds no key and makes none.
    match endpoint_for(Channel::Dev) {
        Err(IpcError::NoEndpointKey { path }) => assert_eq!(path, folder.join("endpoint-key")),
        other => panic!("expected NoEndpointKey, got {other:?}"),
    }

    let mut listener = Listener::bind_keyed(Channel::Dev, &folders).unwrap();
    let key = std::fs::read_to_string(folder.join("endpoint-key")).unwrap();
    let name = listener.endpoint().as_os_str().to_string_lossy();
    #[cfg(windows)]
    assert!(
        name.starts_with(&format!(r"\\.\pipe\{}S-1-5-", Channel::Dev.pipe_prefix()))
            && name.ends_with(&format!("-{key}")),
        "{name}"
    );
    #[cfg(unix)]
    assert!(
        name.ends_with(&format!("-{key}/{}", Channel::Dev.socket_name())),
        "{name}"
    );

    let endpoint = endpoint_for(Channel::Dev).unwrap();
    assert_eq!(&endpoint, listener.endpoint());
    let (client, server) = tokio::join!(wirecatch_ipc::connect(&endpoint), listener.accept());
    client.unwrap();
    server.unwrap();

    drop(listener);
    // The socket's folder in `/tmp` outlives the listener.
    #[cfg(unix)]
    std::fs::remove_dir_all(std::path::Path::new(endpoint.as_os_str()).parent().unwrap()).unwrap();
}
