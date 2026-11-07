mod common;

use common::TestFixture;
use engine::{download, start_share, ReceiveOptions, SecretKey, SendOptions};
use std::time::Duration;

fn receive_as(key: &SecretKey, dir: std::path::PathBuf) -> ReceiveOptions {
    ReceiveOptions {
        output_dir: Some(dir),
        secret_key: Some(key.clone()),
        ..Default::default()
    }
}

/// A single-receiver share serves the first device that connects, turns any
/// other away at once, and lets the first one back in (to resume, say).
#[tokio::test]
async fn e2e_single_receiver_share_serves_only_the_first_device() {
    let fixture = TestFixture::new();
    let content = b"for one device only".to_vec();
    let source = fixture.create_file("one.txt", &content);
    let share = start_share(
        source,
        SendOptions {
            single_receiver: true,
            ..common::send_options()
        },
        None,
        None,
    )
    .await
    .unwrap();

    let first = SecretKey::generate();
    let first_dir = fixture.output_dir().join("first");
    std::fs::create_dir_all(&first_dir).unwrap();
    let (_c1, rx1) = common::no_cancel();
    download(
        share.ticket.clone(),
        receive_as(&first, first_dir.clone()),
        None,
        rx1,
    )
    .await
    .expect("the first device should receive");
    assert_eq!(std::fs::read(first_dir.join("one.txt")).unwrap(), content);

    let other_dir = fixture.output_dir().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let (_c2, rx2) = common::no_cancel();
    let refused = tokio::time::timeout(
        Duration::from_secs(30),
        download(
            share.ticket.clone(),
            receive_as(&SecretKey::generate(), other_dir.clone()),
            None,
            rx2,
        ),
    )
    .await
    .expect("a refused device should fail at once, not keep retrying");
    let error = refused.expect_err("another device must be turned away");
    assert!(
        error.to_string().contains("Another device"),
        "unexpected error: {error}"
    );
    assert!(!other_dir.join("one.txt").exists());

    let again_dir = fixture.output_dir().join("again");
    std::fs::create_dir_all(&again_dir).unwrap();
    let (_c3, rx3) = common::no_cancel();
    download(
        share.ticket.clone(),
        receive_as(&first, again_dir.clone()),
        None,
        rx3,
    )
    .await
    .expect("the first device can connect again with its key");

    drop(share);
}
