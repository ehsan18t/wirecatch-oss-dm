use super::*;
use github::RELEASES_PER_PAGE;
use wirecatch_test_support::http::{Config, TestServer};

fn page_path(page: usize) -> String {
    format!("{LIST}?per_page={RELEASES_PER_PAGE}&page={page}")
}

fn bare(tag: &str) -> Value {
    release(tag, false, &[])
}

impl Harness {
    fn list_page(&self, page: usize, releases: &[Value]) {
        self.server
            .put(&page_path(page), serde_json::to_vec(releases).unwrap());
    }

    async fn failure(&self) -> String {
        let found = self.check(None).await;
        let Err(CoreError::UpdateCheckFailed(message)) = found else {
            panic!("expected a failed check, got {found:?}");
        };
        message
    }
}

#[tokio::test]
async fn a_pre_release_waits_for_the_setting() {
    let h = Harness::github().await;
    h.publish_odd("v0.2.0-beta.1", true, &Odd::default());
    assert_eq!(h.check(Some(kind())).await.unwrap(), None);

    let mut settings = h.manager.get_settings();
    settings.prerelease_updates = true;
    h.manager
        .change_settings(wirecatch_test_support::settings::whole_patch(&settings))
        .unwrap();
    let update = h.offered(Some(kind())).await;
    assert_eq!(update.version, "0.2.0-beta.1");
    assert_eq!(update.download.unwrap().file, installer("v0.2.0-beta.1"));
}

#[tokio::test]
async fn tags_other_than_v_and_a_plain_version_are_ignored() {
    let h = Harness::github().await;
    h.list(&[
        bare("v0.5.0+build.1"),
        bare("vv0.5.0"),
        bare("0.4.0"),
        bare(TAG),
    ]);
    assert_eq!(h.offered(None).await.version, "0.2.0");
}

#[tokio::test]
async fn a_stable_release_after_a_full_first_page_is_found() {
    let h = Harness::github().await;
    let mut first: Vec<_> = (1..RELEASES_PER_PAGE)
        .map(|number| bare(&format!("v0.3.0-beta.{number}")))
        .collect();
    first.push(bare("v0.1.1"));
    h.list_page(1, &first);
    h.list_page(2, &[bare(TAG)]);
    assert_eq!(h.offered(None).await.version, "0.2.0");
}

#[tokio::test]
async fn an_empty_page_completes_the_release_check() {
    let h = Harness::github().await;
    let mut full = vec![bare("v0.1.0"); RELEASES_PER_PAGE];
    full[0] = bare(TAG);
    h.list_page(1, &full);
    h.list_page(2, &[]);
    assert_eq!(h.offered(None).await.version, "0.2.0");
    assert_eq!(h.server.requests(&page_path(2)), 1);
    // A page that was never put is logged under the bare list path.
    assert_eq!(h.server.requests(LIST), 0);
}

#[tokio::test]
async fn a_missing_page_fails_the_check() {
    let h = Harness::github().await;
    h.list_page(1, &vec![bare("v0.1.0"); RELEASES_PER_PAGE]);
    let message = h.failure().await;
    assert!(message.contains("404"), "{message}");
}

#[tokio::test]
async fn a_page_longer_than_asked_for_fails_the_check() {
    let h = Harness::github().await;
    let mut long = vec![bare("v0.1.0"); RELEASES_PER_PAGE + 1];
    long[0] = bare(TAG);
    h.list_page(1, &long);
    let message = h.failure().await;
    assert!(message.contains("too many releases"), "{message}");
}

#[tokio::test]
async fn a_redirected_release_list_fails_the_check() {
    let listed = serde_json::to_vec(&[bare(TAG)]).unwrap();
    let moved = TestServer::start(listed, Config::default()).await;
    let elsewhere = moved.url("/elsewhere/releases").to_string();
    moved.configure(|config| config.redirect = Some((LIST.to_owned(), elsewhere)));
    let repository = FileServer::start().await;
    let releases = AppReleases {
        list: moved.url(LIST),
        repository: repository.url(REPOSITORY),
        verification: ReleaseVerification::Github,
    };
    let h = Harness::with_releases(repository, releases).await;
    let message = h.failure().await;
    assert!(message.contains("redirected"), "{message}");
    assert_eq!(moved.arrived("/elsewhere/releases"), 1);
}
