use muxlane_client::{stream_term, TermUpdate};
use muxlane_core::protocol::b64_encode;
use muxlane_core::protocol::{
    read_frame, write_frame, EventMsg, Request, Response, TermSubscribeResult,
};
use std::sync::{Arc, Mutex};
use tokio::net::UnixListener;

#[tokio::test]
async fn stream_term_handles_replay_resync_and_exit() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("fake.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (r, mut w) = stream.into_split();
        let mut r = tokio::io::BufReader::new(r);
        let req: Request = serde_json::from_value(read_frame(&mut r).await.unwrap()).unwrap();
        let first = b64_encode(b"FIRST");
        write_frame(
            &mut w,
            &Response::ok(
                req.id,
                serde_json::to_value(TermSubscribeResult {
                    sub_id: "s1".into(),
                    replay_b64: first,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        let second = b64_encode(b"SECOND");
        write_frame(
            &mut w,
            &EventMsg::new(
                muxlane_core::protocol::events::TERM_RESYNC,
                serde_json::json!({"agent":"a1","replay_b64":second}),
            ),
        )
        .await
        .unwrap();
        write_frame(
            &mut w,
            &EventMsg::new(
                muxlane_core::protocol::events::TERM_EXIT,
                serde_json::json!({"agent":"a1"}),
            ),
        )
        .await
        .unwrap();
    });
    let updates = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let out = Arc::clone(&updates);
    stream_term(sock.to_str().unwrap(), &"a1".into(), move |update| {
        if let TermUpdate::Resync(bytes) = update {
            out.lock().unwrap().push(bytes);
        }
    })
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(
        *updates.lock().unwrap(),
        vec![b"FIRST".to_vec(), b"SECOND".to_vec()]
    );
}
