//! The app listens where the browser bridge looks for it: at its channel's endpoint, named
//! with the key the app keeps in its local data folder (IPB-10, D41.4).
//!
//! This binary runs this one test, since it points the user's local data folder at a folder
//! of its own through the environment.

use wirecatch_desktop_lib::{bridge, host};
use wirecatch_test_support::private_dir;

// The app listened at a name worked out from the user's SID (Windows) or uid (the `/tmp`
// fallback on Linux) alone, so another account could take it first and keep Wirecatch from
// starting (IPB-10).
#[tokio::test(flavor = "current_thread")]
async fn the_app_listens_at_the_keyed_name_its_clients_read() {
    let dir = private_dir("desktop-endpoint").unwrap();
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

    let listener = bridge::bind().unwrap();
    let key_file = wirecatch_ipc::key_folder(host::CHANNEL)
        .unwrap()
        .join("endpoint-key");
    let key = std::fs::read_to_string(&key_file).unwrap();
    let name = listener.endpoint().as_os_str().to_string_lossy();
    assert!(name.contains(&format!("-{key}")), "{name}");
    let endpoint = host::endpoint().unwrap();
    assert_eq!(&endpoint, listener.endpoint());

    drop(listener);
    // The socket's folder in `/tmp` outlives the listener.
    #[cfg(unix)]
    std::fs::remove_dir_all(std::path::Path::new(endpoint.as_os_str()).parent().unwrap()).unwrap();
}
