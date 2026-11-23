use super::*;

/// The assets of a release whose manifest lists `files`, with an upload of the first one.
fn assets(h: &Harness, files: &[UpdateFile]) -> Vec<Value> {
    let file = &files[0];
    let mut assets = vec![asset(&file.name, file.size, &file.sha256)];
    assets.extend(h.sign(TAG, "0.2.0", files.to_vec(), &key()));
    assets
}

fn no_usable(name: &str) -> String {
    format!("no usable {name}.")
}

#[tokio::test]
async fn a_signed_newer_release_is_offered_from_the_repository_with_the_signed_digest() {
    let h = Harness::signed().await;
    let file = h.publish(TAG);
    let update = h.offered(Some(kind())).await;
    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.page, h.page(TAG));
    let download = update.download.unwrap();
    assert_eq!(download.url, h.server.url(&download_path(TAG, &file.name)));
    assert_eq!(download.file, file);
    assert_eq!(
        std::fs::read(h.fetch(&download).await.unwrap()).unwrap(),
        INSTALLER
    );
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
    for digest in [Value::Null, json!(format!("sha256:{}", sha256(b"other")))] {
        let mut listed = assets(&h, std::slice::from_ref(&file));
        listed[0]["digest"] = digest;
        h.list(&[release(TAG, false, &listed)]);
        let download = h.offered(Some(kind())).await.download.unwrap();
        assert_eq!(download.file, file);
        let path = h.fetch(&download).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
async fn a_newer_release_without_a_manifest_is_refused() {
    let h = Harness::signed().await;
    h.list(&[release(TAG, false, &[])]);
    let message = h.refusal(kind()).await;
    assert!(message.contains(&no_usable(MANIFEST_FILE)), "{message}");
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

#[tokio::test]
async fn signed_releases_refuse_ambiguous_or_incomplete_uploads() {
    let h = Harness::signed().await;
    let assets = assets(&h, &[installer(TAG)]);
    let reasons = [
        "no verifiable installer".to_owned(),
        no_usable(MANIFEST_FILE),
        no_usable(SIGNATURE_FILE),
    ];
    for (index, reason) in reasons.iter().enumerate() {
        let mut duplicate = assets.clone();
        duplicate.push(assets[index].clone());
        h.list(&[release(TAG, false, &duplicate)]);
        let message = h.refusal(kind()).await;
        assert!(message.contains(reason), "{index}: {message}");

        for (field, invalid) in [("state", json!("starter")), ("size", json!(0))] {
            let mut incomplete = assets.clone();
            incomplete[index][field] = invalid;
            h.list(&[release(TAG, false, &incomplete)]);
            let message = h.refusal(kind()).await;
            assert!(message.contains(reason), "{index} {field}: {message}");
        }
    }
}

#[tokio::test]
async fn signed_releases_require_one_target_with_its_expected_filename() {
    let h = Harness::signed().await;
    let file = installer(TAG);
    h.list(&[release(
        TAG,
        false,
        &assets(&h, &[file.clone(), file.clone()]),
    )]);
    let message = h.refusal(kind()).await;
    assert!(message.contains("no verifiable installer"), "{message}");

    let mut renamed = file;
    renamed.name = format!("other-{}", renamed.name);
    h.list(&[release(TAG, false, &assets(&h, &[renamed]))]);
    let message = h.refusal(kind()).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}
