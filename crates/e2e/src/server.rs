//! The HTTP server the scenarios download from: the shared test server of
//! `wirecatch-test-support`, serving ranges with an `ETag` the way the servers
//! browser downloads come from do.

use wirecatch_test_support::http::Config;
pub use wirecatch_test_support::http::TestServer;

/// Serves `data` at every path of a free port on `127.0.0.1`, answering at once.
pub async fn serve(data: Vec<u8>) -> TestServer {
    let config = Config {
        ranges: true,
        etag: Some("\"e2e\"".into()),
        content_type: Some("application/octet-stream".into()),
        ..Config::default()
    };
    TestServer::start(data, config).await
}

/// Like [`serve`], but every answer waits for [`TestServer::release`], so a
/// test can act while the app waits for the server.
pub async fn serve_held(data: Vec<u8>) -> TestServer {
    let server = serve(data).await;
    server.hold();
    server
}
