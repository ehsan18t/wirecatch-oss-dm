//! Frames go both ways over a real connection made with the public API.

use tokio::io::AsyncWriteExt;
use wirecatch_ipc::{Connection, Endpoint, IpcError, Listener, connect};
use wirecatch_protocol::frame::{read_async, write_async};
use wirecatch_protocol::wire::{Call, Event, Message, Outcome, Reply};
use wirecatch_test_support::{PrivateDir, TestResult, private_dir};

/// A fresh endpoint nobody else uses, in a private folder of its own that the
/// returned guard removes.
fn test_endpoint() -> TestResult<(Endpoint, PrivateDir)> {
    let dir = private_dir("ipc")?;
    Ok((Endpoint::from_raw(dir.endpoint_name()), dir))
}

fn assert_send<T: Send + Unpin>(_: &T) {}

#[tokio::test]
async fn requests_responses_and_events_cross_a_connection() {
    let (endpoint, _guard) = test_endpoint().unwrap();
    let mut listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(listener.endpoint(), &endpoint);

    let server = tokio::spawn(async move {
        let mut conn: Connection = listener.accept().await.unwrap();
        while let Some(msg) = read_async::<Message>(&mut conn).await.unwrap() {
            let Message::Request { id, .. } = msg else {
                panic!("expected a request")
            };
            write_async(
                &mut conn,
                &Message::Response {
                    id,
                    outcome: Outcome::Ok(Reply::Done),
                },
            )
            .await
            .unwrap();
            write_async(
                &mut conn,
                &Message::Event {
                    event: Event::QueuesChanged(Vec::new()),
                },
            )
            .await
            .unwrap();
        }
    });

    let mut client = connect(&endpoint).await.unwrap();
    assert_send(&client);
    for id in 1..=3 {
        write_async(
            &mut client,
            &Message::Request {
                id,
                call: Call::PauseAll,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            read_async::<Message>(&mut client).await.unwrap(),
            Some(Message::Response {
                id,
                outcome: Outcome::Ok(Reply::Done)
            })
        );
        assert_eq!(
            read_async::<Message>(&mut client).await.unwrap(),
            Some(Message::Event {
                event: Event::QueuesChanged(Vec::new())
            })
        );
    }
    client.shutdown().await.unwrap();
    drop(client);
    server.await.unwrap();
}

#[tokio::test]
async fn several_clients_are_served_one_after_another() {
    let (endpoint, _guard) = test_endpoint().unwrap();
    let mut listener = Listener::bind(&endpoint).unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let mut conn = listener.accept().await.unwrap();
            let msg: Message = read_async(&mut conn).await.unwrap().unwrap();
            write_async(&mut conn, &msg).await.unwrap();
        }
    });
    for id in 0..3 {
        let mut client = connect(&endpoint).await.unwrap();
        let msg = Message::Request {
            id,
            call: Call::ResumeAll,
        };
        write_async(&mut client, &msg).await.unwrap();
        assert_eq!(read_async::<Message>(&mut client).await.unwrap(), Some(msg));
    }
    server.await.unwrap();
}

#[tokio::test]
async fn connecting_without_a_listener_reports_not_running() {
    let (endpoint, _guard) = test_endpoint().unwrap();
    assert!(matches!(
        connect(&endpoint).await,
        Err(IpcError::NotRunning { .. })
    ));
}

#[tokio::test]
async fn the_real_endpoint_is_per_user() {
    let endpoint = wirecatch_ipc::endpoint().unwrap();
    let name = endpoint.as_os_str().to_string_lossy().into_owned();
    let release = wirecatch_ipc::Channel::Release;
    #[cfg(windows)]
    assert!(
        name.starts_with(&format!(r"\\.\pipe\{}S-1-5-", release.pipe_prefix())),
        "{name}"
    );
    #[cfg(unix)]
    assert!(
        name.ends_with(&format!("/{}", release.socket_name())),
        "{name}"
    );
}
