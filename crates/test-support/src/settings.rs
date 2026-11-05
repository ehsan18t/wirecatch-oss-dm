//! Settings for tests that change a few fields of the whole.

use wirecatch_protocol::app::{
    AppSettings, AppSettingsPatch, MediaDefaults, MediaDefaultsPatch, OutputDefaults,
    OutputDefaultsPatch, Update,
};
use wirecatch_protocol::extension::{ExtensionSettings, ExtensionSettingsPatch};

/// The patch that makes the settings exactly `settings`. Every field is named on both sides, so a
/// setting added to one and not the other does not compile.
#[must_use]
pub fn whole_patch(settings: &AppSettings) -> AppSettingsPatch {
    let AppSettings {
        default_folder,
        categories,
        connections_per_download,
        max_concurrent_downloads,
        global_speed_limit,
        extension,
        theme,
        accent,
        clock,
        list_lines,
        start_with_system,
        close_to_tray,
        notify_on_finish,
        finish_notice_time,
        notify_on_start,
        start_notice_time,
        notify_during_do_not_disturb,
        auto_update_addons,
        show_progress_windows,
        when_file_exists,
        media,
        local_sites,
        update_suffix_list,
        prerelease_updates,
        proxy,
    } = settings.clone();
    AppSettingsPatch {
        default_folder: Some(default_folder),
        categories: Some(categories),
        connections_per_download: Some(connections_per_download),
        max_concurrent_downloads: Some(max_concurrent_downloads),
        global_speed_limit: Update::Set(global_speed_limit),
        extension: Some(extension_patch(extension)),
        theme: Some(theme),
        accent: Some(accent),
        clock: Some(clock),
        list_lines: Some(list_lines),
        start_with_system: Some(start_with_system),
        close_to_tray: Some(close_to_tray),
        notify_on_finish: Some(notify_on_finish),
        finish_notice_time: Some(finish_notice_time),
        notify_on_start: Some(notify_on_start),
        start_notice_time: Some(start_notice_time),
        notify_during_do_not_disturb: Some(notify_during_do_not_disturb),
        auto_update_addons: Some(auto_update_addons),
        show_progress_windows: Some(show_progress_windows),
        when_file_exists: Some(when_file_exists),
        media: Some(media_patch(media)),
        local_sites: Some(local_sites),
        update_suffix_list: Some(update_suffix_list),
        prerelease_updates: Some(prerelease_updates),
        proxy: Some(proxy),
    }
}

fn extension_patch(extension: ExtensionSettings) -> ExtensionSettingsPatch {
    let ExtensionSettings {
        capture_enabled,
        ask_before_download,
        show_media_button,
        ignored_sites,
        capture_extensions,
        min_capture_size,
        default_queue,
        video_sites,
        private_windows,
    } = extension;
    ExtensionSettingsPatch {
        capture_enabled: Some(capture_enabled),
        ask_before_download: Some(ask_before_download),
        show_media_button: Some(show_media_button),
        ignored_sites: Some(ignored_sites),
        add_ignored_sites: None,
        remove_ignored_sites: None,
        capture_extensions: Some(capture_extensions),
        min_capture_size: Some(min_capture_size),
        default_queue: Some(default_queue),
        video_sites: Some(video_sites),
        private_windows: Some(private_windows),
    }
}

fn media_patch(media: MediaDefaults) -> MediaDefaultsPatch {
    let MediaDefaults {
        video,
        audio,
        ask_each_time,
        remember_choices,
        quality,
        fallback,
    } = media;
    MediaDefaultsPatch {
        video: Some(output_patch(video)),
        audio: Some(output_patch(audio)),
        ask_each_time: Some(ask_each_time),
        remember_choices: Some(remember_choices),
        quality: Some(quality),
        fallback: Some(fallback),
    }
}

fn output_patch(output: OutputDefaults) -> OutputDefaultsPatch {
    let OutputDefaults {
        format,
        audio_kbps,
        subtitle_languages,
        auto_subtitles,
        subtitles,
        unembedded_subtitles,
        chapters,
    } = output;
    OutputDefaultsPatch {
        format: Some(format),
        audio_kbps: Update::Set(audio_kbps),
        subtitle_languages: Some(subtitle_languages),
        auto_subtitles: Some(auto_subtitles),
        subtitles: Some(subtitles),
        unembedded_subtitles: Some(unembedded_subtitles),
        chapters: Some(chapters),
    }
}
