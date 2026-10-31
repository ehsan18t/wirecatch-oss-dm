//! Which addresses a page's formats may lead to (D34, D39).

use std::net::IpAddr;
use std::time::Duration;

use wirecatch_engine::{PrivateNetwork, Url};
use wirecatch_media::describe;
use wirecatch_protocol::app::UserError;
use wirecatch_sites::local_network::{self, ChosenBy};

/// How a format's address may use the local network. A page that is itself on
/// the local network (a media server at home) may point anywhere. A page on the
/// internet may only point to the internet: a format whose host is local, or
/// whose name does not resolve (so cannot be checked), is refused early with a
/// clear message, and the rest download with the local network blocked for
/// every request, redirect hop and connection. That block, not this early
/// check, is what keeps a format out of the local network, so a redirect or a
/// name that answers local only when the engine connects (DNS rebinding) is
/// refused too.
///
/// When yt-dlp downloads a format by itself (the fallback), its own requests
/// are not covered by this.
pub(super) async fn network_rule(
    chosen_by: ChosenBy,
    page: &Url,
    format: &Url,
) -> Result<PrivateNetwork, UserError> {
    if page_is_local(chosen_by, page).await {
        return Ok(PrivateNetwork::Allow);
    }
    format_rule(chosen_by, &local_network::addresses(format, LOOKUP).await)
}

/// How long the early check waits for a format's name to resolve.
pub(super) const LOOKUP: Duration = Duration::from_secs(10);

/// Whether a video page is on the local network. A page the person chose is
/// where its name leads for them, as [`PrivateNetwork::Auto`] decides for a
/// link they typed. A page on their network by its own address or their list
/// of sites (D39) is. A website's page never is: where its name resolves is
/// for the site's owner to answer, who could make it answer local for this
/// one lookup (CORE-3).
pub(super) async fn page_is_local(chosen_by: ChosenBy, page: &Url) -> bool {
    match chosen_by {
        ChosenBy::Person => {
            let found = local_network::addresses(page, LOOKUP).await;
            !found.is_empty() && found.into_iter().all(local_network::is_local)
        }
        ChosenBy::LocalPage => true,
        ChosenBy::Website => false,
    }
}

/// The rule for a format of a page on the internet, chosen by `chosen_by`,
/// whose host has the addresses `found`.
pub(super) fn format_rule(
    chosen_by: ChosenBy,
    found: &[IpAddr],
) -> Result<PrivateNetwork, UserError> {
    if found.is_empty() {
        return Err(describe::format_server_not_found());
    }
    if found.iter().copied().any(local_network::is_local) {
        return Err(wirecatch_messages::private_network(chosen_by));
    }
    Ok(PrivateNetwork::Block)
}
