//! Sends one browser capture to a running Wirecatch Dev, the way the extension does, so the
//! questions it asks (confirm a download, pick a quality, install the media add-on) can be tried
//! without a browser. Development only: it talks to the debug build's endpoint.
//!
//! ```text
//! cargo run -p wirecatch-desktop --example send_capture -- page https://commons.wikimedia.org/wiki/File:Example.webm
//! cargo run -p wirecatch-desktop --example send_capture -- hls https://example.com/master.m3u8
//! cargo run -p wirecatch-desktop --example send_capture -- file https://example.com/a.zip
//! ```

use wirecatch_desktop_lib::host;
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::extension::{CaptureSource, MediaHint, MediaKind, NewDownload};
use wirecatch_protocol::frame::{read_async, write_async};
use wirecatch_protocol::wire::{Call, ClientKind, Hello, Message};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [kind, url] = args.as_slice() else {
        return Err("usage: send_capture <page|hls|dash|file> <url>".into());
    };
    let media = match kind.as_str() {
        "page" => Some(MediaKind::Page),
        "hls" => Some(MediaKind::Hls),
        "dash" => Some(MediaKind::Dash),
        "file" => None,
        other => return Err(format!("unknown kind {other}: use page, hls, dash or file").into()),
    };
    let endpoint = host::endpoint()?;
    let mut conn = wirecatch_ipc::connect(&endpoint).await?;
    let hello = Call::Hello(Hello {
        client: ClientKind::Cli,
        client_version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        browser: None,
    });
    let handover_id = format!("send-capture-{}", std::process::id());
    let capture = Call::AddDownload(NewDownload {
        handover_id,
        url: url.clone(),
        referrer: None,
        page_url: Some(url.clone()),
        suggested_name: None,
        mime: None,
        size_hint: None,
        cookies: None,
        user_agent: None,
        extra_headers: Vec::new(),
        source: CaptureSource::MediaButton,
        queue: None,
        media: media.map(|kind| MediaHint { kind, title: None }),
    });
    for (id, call) in [(1, hello), (2, capture)] {
        write_async(&mut conn, &Message::Request { id, call }).await?;
        loop {
            match read_async::<Message>(&mut conn).await? {
                Some(Message::Response {
                    id: answered,
                    outcome,
                }) if answered == id => {
                    println!("{outcome:?}");
                    break;
                }
                Some(_) => continue,
                None => return Err("Wirecatch Dev closed the connection".into()),
            }
        }
    }
    Ok(())
}
