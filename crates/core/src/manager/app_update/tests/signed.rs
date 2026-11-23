use super::*;

#[tokio::test]
async fn a_signed_newer_release_is_offered_with_the_signed_digest() {
    let h = Harness::signed().await;
    let file = h.publish(TAG);
    let update = h.offered(Some(kind())).await;
    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.download.unwrap().file, file);
    assert_eq!(
        h.manager
            .check_app_update("0.2.0", Some(kind()))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn signed_releases_ignore_the_digest_github_reports() {
    let h = Harness::signed().await;
    let file = installer(TAG);
    h.server
        .put(&download_path(TAG, &file.name), INSTALLER.to_vec());
    let mut listed = vec![h.asset(TAG, &file.name, file.size, &sha256(b"other"))];
    listed.extend(h.sign(TAG, "0.2.0", vec![file.clone()], &key()));
    h.list(&[release(TAG, false, &listed)]);
    let download = h.offered(Some(kind())).await.download.unwrap();
    assert_eq!(download.file, file);
    let path = h.fetch(&download).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);
}

#[tokio::test]
async fn a_manifest_entry_with_another_name_is_refused() {
    let h = Harness::signed().await;
    let file = installer(TAG);
    let mut renamed = file.clone();
    renamed.name = format!("../{}", file.name);
    let mut listed = vec![h.asset(TAG, &file.name, file.size, &file.sha256)];
    listed.extend(h.sign(TAG, "0.2.0", vec![renamed], &key()));
    h.list(&[release(TAG, false, &listed)]);
    let message = h.refusal(kind()).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}

#[tokio::test]
async fn a_newer_release_without_a_manifest_is_refused() {
    let h = Harness::signed().await;
    h.list(&[release(TAG, false, &[])]);
    let message = h.refusal(kind()).await;
    assert!(
        message.contains(&format!("no {MANIFEST_FILE}")),
        "{message}"
    );
}

#[tokio::test]
async fn a_release_signed_with_another_key_is_not_offered() {
    let h = Harness::signed().await;
    let odd = Odd {
        signer: Some(SigningKey::from_bytes(&[8; 32])),
        ..Odd::default()
    };
    h.publish_odd(TAG, false, &odd);
    let message = h.refusal(kind()).await;
    assert!(message.contains("signature does not match"), "{message}");
}

#[tokio::test]
async fn a_manifest_naming_another_version_is_refused() {
    let h = Harness::signed().await;
    let odd = Odd {
        manifest_version: Some("0.3.0"),
        ..Odd::default()
    };
    h.publish_odd(TAG, false, &odd);
    let message = h.refusal(kind()).await;
    assert!(message.contains("names version 0.3.0"), "{message}");
}

#[tokio::test]
async fn an_asset_of_another_size_than_its_manifest_is_refused() {
    let h = Harness::signed().await;
    let odd = Odd {
        asset_size: Some(INSTALLER.len() as u64 + 1),
        ..Odd::default()
    };
    h.publish_odd(TAG, false, &odd);
    let message = h.refusal(kind()).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}
