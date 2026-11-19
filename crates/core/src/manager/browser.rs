//! What the extension asks of the app as a shell (D279): the values its checks use, the popup's
//! options and page row, its actions, and the app's decision on each capture.

use std::sync::atomic::Ordering;

use wirecatch_product::PRODUCT_NAME;
use wirecatch_protocol::app::{AppSettings, AppSettingsPatch};
use wirecatch_protocol::extension::{
    CaptureSource, ExtensionSettingsPatch, normalize_file_type, normalize_site,
};
use wirecatch_protocol::hosts::host_on_site;
use wirecatch_protocol::shell::{
    CheckAnswer, CookiePlan, DownloadCheck, InvokeAction, OptionList, PageOptions, PageQuery,
    Revision, SetOption, ShellConfig, actions,
};
use wirecatch_protocol::wire::Event;
use wirecatch_sites::cookie_scope::{is_cross_site, site_of};

use super::Manager;
use super::options::{option_groups, option_patch, shell_config};
use super::state::State;
use crate::error::{CoreError, Result};

impl Manager {
    /// The revision of what the app sends the extension now.
    fn revision(&self) -> Revision {
        Revision {
            instance: self.inner.instance.clone(),
            counter: self.inner.revision.load(Ordering::Relaxed),
        }
    }

    /// The values the extension's own checks use.
    #[must_use]
    pub fn shell_config(&self) -> ShellConfig {
        shell_config(&self.lock().settings, self.revision())
    }

    /// The options the extension's popup shows.
    #[must_use]
    pub fn options(&self) -> OptionList {
        let state = self.lock();
        self.option_list(&state)
    }

    fn option_list(&self, state: &State) -> OptionList {
        OptionList {
            revision: self.revision(),
            groups: option_groups(&state.settings, &state.queue_summaries()),
        }
    }

    /// Whether settings going from `old` to those in `state` change what the extension shows or
    /// checks.
    pub(super) fn shell_differs(&self, old: &AppSettings, state: &State) -> bool {
        let revision = self.revision();
        let queues = state.queue_summaries();
        shell_config(old, revision.clone()) != shell_config(&state.settings, revision)
            || option_groups(old, &queues) != option_groups(&state.settings, &queues)
    }

    /// Tells every connected browser that the options, and the values its checks use, may have
    /// changed: under a new revision, so a late answer from before never wins.
    pub(super) fn publish_shell(&self, state: &State) {
        self.inner.revision.fetch_add(1, Ordering::Relaxed);
        self.emit_wire(Event::ShellConfigChanged(Box::new(shell_config(
            &state.settings,
            self.revision(),
        ))));
        self.emit_wire(Event::OptionsChanged(Box::new(self.option_list(state))));
    }

    /// Changes one option from the popup, checked like any change made in the app, and returns
    /// the options as they are now.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for an unknown option or a value that does not fit it, and the
    /// errors of [`change_settings`](Self::change_settings).
    pub fn set_option(&self, set: SetOption) -> Result<OptionList> {
        self.change_settings_by(|settings| option_patch(settings, &set.id, set.change))?;
        Ok(self.options())
    }

    /// Runs an action the popup offers.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for an unknown action or one about a page that names no site, and
    /// the errors of [`change_settings`](Self::change_settings).
    pub fn invoke_action(&self, action: &InvokeAction) -> Result<()> {
        let site = || {
            action
                .site
                .as_deref()
                .and_then(normalize_site)
                .ok_or_else(|| CoreError::Invalid("That page has no site to ignore.".into()))
        };
        let take = match action.id.as_str() {
            actions::IGNORE_SITE => false,
            actions::TAKE_SITE => true,
            other => {
                return Err(CoreError::Invalid(format!(
                    "This version of {PRODUCT_NAME} cannot do \"{other}\". Update {PRODUCT_NAME}."
                )));
            }
        };
        let host = site()?;
        self.change_settings_by(|settings| {
            let patch = if take {
                // Every entry that covers the page, so the page is taken whichever entry ignored it.
                let entries = settings
                    .extension
                    .ignored_sites
                    .iter()
                    .filter(|entry| host_on_site(&host, entry))
                    .cloned()
                    .collect();
                ExtensionSettingsPatch {
                    remove_ignored_sites: Some(entries),
                    ..ExtensionSettingsPatch::default()
                }
            } else {
                ExtensionSettingsPatch {
                    add_ignored_sites: Some(vec![page_site(&host)]),
                    ..ExtensionSettingsPatch::default()
                }
            };
            wirecatch_settings::check_site_patch(&patch)?;
            Ok(AppSettingsPatch {
                extension: Some(patch),
                ..AppSettingsPatch::default()
            })
        })
    }

    /// What the popup shows about a page on `host`: the entry that ignores it, or its own site.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for a host name that names no site.
    pub fn page_options(&self, query: &PageQuery) -> Result<PageOptions> {
        let host = normalize_site(&query.host)
            .ok_or_else(|| CoreError::Invalid("That page has no site to ignore.".into()))?;
        let state = self.lock();
        let entry = state
            .settings
            .extension
            .ignored_sites
            .iter()
            .find(|entry| host_on_site(&host, entry));
        Ok(match entry {
            Some(entry) => PageOptions {
                site: entry.clone(),
                ignored: true,
            },
            None => PageOptions {
                site: page_site(&host),
                ignored: false,
            },
        })
    }

    /// Whether the app takes a capture, decided before the extension reads any cookie, and which
    /// cookies it reads when it does.
    #[must_use]
    pub fn check_download(&self, check: &DownloadCheck) -> CheckAnswer {
        let state = self.lock();
        if let Some(reason) = leave_reason(&state, check) {
            return CheckAnswer::Leave {
                reason: reason.into(),
            };
        }
        if check
            .private_session
            .as_deref()
            .is_some_and(|session| self.private_session_ended(session))
        {
            return CheckAnswer::Leave {
                reason: "private_session_ended".into(),
            };
        }
        let request = &check.cookie_request;
        let site = site_of(&check.url, &state.suffixes);
        let top_level_site = if request.navigation {
            site.clone()
        } else {
            request
                .top_level
                .as_deref()
                .and_then(|page| site_of(page, &state.suffixes))
        };
        CheckAnswer::Take {
            cookies: CookiePlan {
                cross_site: is_cross_site(&check.url, request, &state.suffixes),
                site,
                top_level_site,
            },
        }
    }
}

/// The site "Ignore this site" adds for a page on `host`: the host without `www.`.
fn page_site(host: &str) -> String {
    host.strip_prefix("www.").unwrap_or(host).to_owned()
}

/// Why the settings leave a capture in the browser, the same checks the extension makes first.
/// The capture filters are about the browser's own downloads: a link the person sends from the
/// menu or the media button goes whatever they say.
fn leave_reason(state: &State, check: &DownloadCheck) -> Option<&'static str> {
    let extension = &state.settings.extension;
    if check.private_session.is_some() && !extension.private_windows {
        return Some("private_window");
    }
    if check.source != CaptureSource::BrowserDownload {
        return None;
    }
    if !extension.capture_enabled {
        return Some("capture_off");
    }
    let addresses = [
        Some(check.url.as_str()),
        check.cookie_request.original_url.as_deref(),
        check.page_url.as_deref(),
    ];
    let ignored = addresses.into_iter().flatten().any(|address| {
        wirecatch_engine::Url::parse(address)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .is_some_and(|host| {
                extension
                    .ignored_sites
                    .iter()
                    .any(|entry| host_on_site(host.trim_end_matches('.'), entry))
            })
    });
    if ignored {
        return Some("ignored_site");
    }
    let wanted: Vec<String> = extension
        .capture_extensions
        .iter()
        .filter_map(|kind| normalize_file_type(kind))
        .collect();
    if !wanted.is_empty() {
        let name = check
            .file_name
            .clone()
            .or_else(|| file_name_of(&check.url))
            .map(|name| name.to_lowercase());
        let matches = name.is_some_and(|name| {
            wanted
                .iter()
                .any(|kind| name.ends_with(&format!(".{kind}")))
        });
        if !matches {
            return Some("file_type");
        }
    }
    if check
        .size
        .is_some_and(|size| size < extension.min_capture_size)
    {
        return Some("too_small");
    }
    None
}

/// The last segment of an address's path, decoded, or `None` when there is none.
fn file_name_of(address: &str) -> Option<String> {
    let url = wirecatch_engine::Url::parse(address).ok()?;
    let last = url.path_segments()?.next_back()?;
    if last.is_empty() {
        return None;
    }
    Some(
        percent_encoding::percent_decode_str(last)
            .decode_utf8()
            .map_or_else(|_| last.to_owned(), std::borrow::Cow::into_owned),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_site_drops_www() {
        assert_eq!(page_site("www.example.com"), "example.com");
        assert_eq!(page_site("cdn.example.com"), "cdn.example.com");
    }

    #[test]
    fn a_file_name_is_read_from_the_last_path_segment() {
        assert_eq!(
            file_name_of("https://example.com/a/b%20c.zip?x=1").as_deref(),
            Some("b c.zip")
        );
        assert_eq!(file_name_of("https://example.com/"), None);
    }
}
