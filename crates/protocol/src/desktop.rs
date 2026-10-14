//! The desktop app's own boundary: the commands its interface calls in the Rust shell, the
//! events the shell sends it, and the address of a download window's page.
//!
//! Every command is declared once here with its arguments and its answer. The shell registers
//! exactly these (a test in `apps/desktop/src-tauri` holds it to [`COMMAND_NAMES`]), each taking
//! its arguments as one `args` parameter of the command's `Args` type (for example
//! [`commands::add_download::Args`]) and answering with its `Output`, and the interface calls them through [`CommandMap`], generated for TypeScript, so a
//! renamed command, argument or answer fails to compile on the side that did not follow. A
//! command fails with a [`ProtocolError`](crate::wire::ProtocolError): a code the interface
//! acts on, never the wording, and a message for a person.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{
    AddRequest, AddonId, AddonInfo, AddonRequestItem, AppSettings, AppSettingsPatch,
    CaptureConfirmation, DownloadSummary, ImportPreview, MediaChoice, MediaSelection,
    PendingCapture, Question, Queue, StartupNotice, SuffixListInfo,
};
use crate::secret::Link;
use crate::{DownloadId, QuestionId, QueueId};

crate::exported::exported_consts! {
    /// The Tauri event that carries every [`UiEvent`](crate::app::UiEvent), in order.
    pub const UI_EVENT: &str = "wirecatch://event";
    /// The Tauri event, with no payload, that says events were missed: reload everything.
    pub const RESYNC_EVENT: &str = "wirecatch://resync";
    /// The Tauri event, with no payload, that says links arrived from a second launch: fetch
    /// them with `take_launch_links`.
    pub const LINKS_EVENT: &str = "wirecatch://links";
    /// The Tauri event, sent to one window, that carries its [`WindowFrame`] after it changed.
    pub const WINDOW_FRAME_EVENT: &str = "wirecatch://window-frame";
    /// The Tauri event, sent to every window, that carries the [`SystemAppearance`] after the
    /// system changed it.
    pub const SYSTEM_APPEARANCE_EVENT: &str = "wirecatch://system-appearance";
    /// The page a download window shows, relative to the app's address.
    pub const DOWNLOAD_WINDOW_PAGE: &str = "download.html";
    /// The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the question a window asks.
    pub const QUESTION_PARAMETER: &str = "question";
    /// The query parameter of [`DOWNLOAD_WINDOW_PAGE`] naming the download a window shows.
    pub const DOWNLOAD_PARAMETER: &str = "download";
    /// File types that open directly, without asking first: common video, audio, image, document
    /// and archive types that do not run code when opened. Every other type, and a file with no
    /// extension, could run code (programs, installers, scripts, shortcuts, disk images, Office
    /// files with macros and many more), so opening it needs the person's confirmation. An
    /// allowlist fails safe: a type nobody thought of asks first.
    ///
    /// In order: video, audio, subtitles, images, documents, archives. Left out on purpose among
    /// documents: Office files with macros (`docm` and friends), `rtf` (embedded objects), the
    /// OpenDocument formats (macros) and `csv` (formulas that run when a spreadsheet opens it).
    /// `docx`, `xlsx` and `pptx` stay: they cannot hold macros, and Office opens downloaded ones,
    /// which carry the mark of the web, in Protected View.
    ///
    /// The shell decides with it (`is_program` in `apps/desktop/src-tauri/src/api.rs`) and asks in
    /// a warning of its own; the sample backend reads it to ask the same way.
    pub const SAFE_EXTENSIONS: &[&str] = &[
        "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "3gp",
        "ogv", "mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aiff", "mka", "srt",
        "vtt", "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
        "pdf", "txt", "md", "docx", "xlsx", "pptx", "epub", "zip", "7z", "rar", "tar", "gz", "tgz",
        "bz2", "xz", "zst",
    ];
}

/// What a window's own caption buttons ask of it. Every window is frameless (D36), so its title
/// bar is part of its page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WindowAction {
    /// Minimize it.
    Minimize,
    /// Maximize it, or restore it when it is maximized.
    ToggleMaximize,
    /// Close it, as its X does: the main window may go to the tray.
    Close,
}

/// The state of a window that its title bar shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WindowFrame {
    /// It fills the screen; the maximize button shows restore.
    pub maximized: bool,
    /// It has the keyboard focus; an unfocused title bar is drawn quieter.
    pub focused: bool,
    /// The pointer is over the maximize button. On Windows the shell, not the page, receives
    /// the pointer there (so the system can offer its snap layouts), and says so here.
    pub maximize_hovered: bool,
}

/// Where a window's maximize button is, in CSS pixels from the top left of its page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ButtonArea {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// What the operating system says about how apps should look, beyond light or dark (which the
/// page reads from `prefers-color-scheme`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SystemAppearance {
    /// The system's accent colour as `#rrggbb`, when it has one (Windows); `null` elsewhere,
    /// where the system accent falls back to Wirecatch's cyan.
    pub accent: Option<String>,
}

/// A command's name, as the interface invokes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandName(pub &'static str);

/// One command's arguments and answer, as the TypeScript side reads them; never built.
#[derive(TS)]
#[ts(export)]
pub struct Command<A, O> {
    /// What the interface sends, as the command's `args` parameter; `null` when it takes none.
    pub args: A,
    /// What the command answers with.
    pub output: O,
}

/// The arguments of a command, or `()` for one that takes none.
macro_rules! args_or_unit {
    () => {
        ()
    };
    ($args:ident) => {
        $args
    };
}

/// Declares every command: its arguments struct, a module named after it with its name, its
/// `Args` and its `Output`, the list of names, and the map the TypeScript side calls through.
macro_rules! commands {
    ($(
        $(#[doc = $doc:literal])+
        $name:ident $( : $args:ident { $( $(#[doc = $pdoc:literal])+ $param:ident : $pty:ty ),+ $(,)? } )? -> $out:ty;
    )+) => {
        $($(
            #[doc = concat!(" The arguments of `", stringify!($name), "`.")]
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
            #[ts(export)]
            pub struct $args {
                $( $(#[doc = $pdoc])+ pub $param: $pty, )+
            }
        )?)+

        /// One module per command, named as the command is.
        pub mod commands {
            $(
                $(#[doc = $doc])+
                pub mod $name {
                    use super::super::*;

                    /// The command's name, as the interface invokes it.
                    pub const NAME: CommandName = CommandName(stringify!($name));
                    $(
                        /// What the interface sends, as the command's `args` parameter.
                        pub type Args = $args;
                    )?
                    /// What the command answers with.
                    pub type Output = $out;
                }
            )+
        }

        /// Every command's name, in the order they are declared.
        pub const COMMAND_NAMES: &[CommandName] = &[$(commands::$name::NAME),+];

        /// Every command, with its arguments and answer, for the TypeScript side.
        #[derive(TS)]
        #[ts(export)]
        pub struct CommandMap {
            $(
                $(#[doc = $doc])+
                pub $name: Command<args_or_unit!($($args)?), $out>,
            )+
        }
    };
}

commands! {
    /// Every download, in queue order.
    list_downloads -> Vec<DownloadSummary>;
    /// Adds a download from the app (paste, Add dialog, a link passed on the command line).
    add_download: AddDownloadArgs {
        /// The download to add.
        request: AddRequest,
    } -> DownloadId;
    /// Pauses one download, keeping what it has downloaded.
    pause: PauseArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Lets a paused or failed download continue.
    resume: ResumeArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Stops a download and discards its partial file. The row stays, so it can start again.
    cancel: CancelArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Removes downloads from the list, and their files when asked.
    remove: RemoveArgs {
        /// The downloads.
        ids: Vec<DownloadId>,
        /// Delete their files too.
        delete_file: bool,
    } -> ();
    /// Starts a failed or cancelled download again.
    retry: RetryArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Swaps in a fresh link for an expired one, keeping the bytes already downloaded.
    refresh_link: RefreshLinkArgs {
        /// The download.
        id: DownloadId,
        /// The new link.
        url: Link,
    } -> ();
    /// Saves a video whose conversion failed as it was downloaded instead (D24).
    keep_original: KeepOriginalArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Moves downloads to another queue.
    move_to_queue: MoveToQueueArgs {
        /// The downloads.
        ids: Vec<DownloadId>,
        /// The queue they move to.
        queue: QueueId,
    } -> ();
    /// Puts a queue's downloads in this order.
    reorder: ReorderArgs {
        /// The queue.
        queue: QueueId,
        /// Its downloads in their new order.
        ids_in_order: Vec<DownloadId>,
    } -> ();
    /// Pauses every download that is running or waiting.
    pause_all -> ();
    /// Puts every paused download back in line.
    resume_all -> ();
    /// Every queue, in sidebar order.
    list_queues -> Vec<Queue>;
    /// Makes a new queue.
    create_queue: CreateQueueArgs {
        /// Its name.
        name: String,
    } -> Queue;
    /// Saves a queue's name, limits and schedule.
    update_queue: UpdateQueueArgs {
        /// The queue as it should be.
        queue: Queue,
    } -> ();
    /// Deletes a queue; its downloads move to the default queue.
    delete_queue: DeleteQueueArgs {
        /// The queue.
        id: QueueId,
    } -> ();
    /// Pauses or resumes a whole queue. A paused queue starts nothing new and pauses what it
    /// was running.
    set_queue_paused: SetQueuePausedArgs {
        /// The queue.
        id: QueueId,
        /// Paused or not.
        paused: bool,
    } -> ();
    /// Runs a scheduled queue now, outside its hours, until it is empty or paused.
    start_queue_now: StartQueueNowArgs {
        /// The queue.
        id: QueueId,
    } -> ();
    /// Every setting.
    get_settings -> AppSettings;
    /// Changes only the settings in the patch, applied to the app's current settings.
    change_settings: ChangeSettingsArgs {
        /// The settings that change.
        patch: AppSettingsPatch,
    } -> ();
    /// Writes the settings profile to a file.
    export_settings: ExportSettingsArgs {
        /// The file to write.
        path: String,
    } -> ();
    /// What importing a settings profile would change, without changing anything.
    preview_import: PreviewImportArgs {
        /// The profile to read.
        path: String,
    } -> ImportPreview;
    /// Reads a settings profile and applies it.
    import_settings: ImportSettingsArgs {
        /// The profile to read.
        path: String,
    } -> ();
    /// Browser downloads waiting for the person to confirm them.
    pending_confirmations -> Vec<PendingCapture>;
    /// Starts a confirmed capture, or with `later` adds it paused; answers with the new
    /// download. A download window that asked becomes its progress window.
    confirm_capture: ConfirmCaptureArgs {
        /// The question.
        id: QuestionId,
        /// The person's answer.
        confirmation: CaptureConfirmation,
    } -> DownloadId;
    /// Drops a captured download the person did not want.
    reject_capture: RejectCaptureArgs {
        /// The question.
        id: QuestionId,
    } -> ();
    /// Opens a finished file. A program (an installer, a script, a shortcut) opens only once the
    /// person agreed to the shell's own warning, which names where it came from; the page cannot
    /// answer that warning, so declining it is not an error.
    open_file: OpenFileArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Shows a download's file in its folder, or the folder alone while the file is not there
    /// yet.
    open_folder: OpenFolderArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Moves a download, running or not, to another folder.
    set_download_folder: SetDownloadFolderArgs {
        /// The download.
        id: DownloadId,
        /// The folder it moves to.
        folder: String,
    } -> ();
    /// Links passed on the command line that the interface has not shown yet, handed over once.
    take_launch_links -> Vec<String>;
    /// Downloads the chosen quality, or with `later` adds it paused; answers with the new
    /// download, like `confirm_capture`. Fails with `addon_required` while the option needs
    /// the media add-on, which the core then asks for and keeps the choice open.
    choose_media: ChooseMediaArgs {
        /// The question.
        id: QuestionId,
        /// The person's pick.
        selection: MediaSelection,
    } -> DownloadId;
    /// Drops a media choice the person did not want, media still being read, or a page waiting
    /// for the add-on.
    dismiss_media_choice: DismissMediaChoiceArgs {
        /// The question.
        id: QuestionId,
    } -> ();
    /// Media choices still open, so a new or reloaded window asks them again.
    pending_media_choices -> Vec<MediaChoice>;
    /// Pages waiting for an add-on, oldest first, so a new or reloaded window offers it again.
    pending_addon_requests -> Vec<AddonRequestItem>;
    /// Every add-on and its state (D2, D20).
    list_addons -> Vec<AddonInfo>;
    /// Installs an add-on in the background; progress and the result arrive as
    /// `AddonsChanged`.
    install_addon: InstallAddonArgs {
        /// The add-on.
        id: AddonId,
    } -> ();
    /// Brings an add-on up to date in the background, like `install_addon`.
    update_addon: UpdateAddonArgs {
        /// The add-on.
        id: AddonId,
    } -> ();
    /// Removes an add-on and its files, stopping an install in progress first.
    remove_addon: RemoveAddonArgs {
        /// The add-on.
        id: AddonId,
    } -> ();
    /// Stops an install or update; an update keeps the installed version working.
    cancel_addon_operation: CancelAddonOperationArgs {
        /// The add-on.
        id: AddonId,
    } -> ();
    /// Looks for a newer media add-on now; answers with the add-ons as they stand afterwards.
    check_addon_updates -> Vec<AddonInfo>;
    /// The Public Suffix List in use and when the app last looked for a newer one (D44).
    suffix_list -> SuffixListInfo;
    /// Looks for a newer Public Suffix List now, whatever
    /// [`AppSettings::update_suffix_list`](crate::app::AppSettings::update_suffix_list) says, and takes it when it passes every check;
    /// answers with the list as it stands afterwards. Fails with `update_check_failed` and
    /// a message for the person when it could not be fetched or was refused, keeping the
    /// list in use.
    check_suffix_list -> SuffixListInfo;
    /// The open question a download window shows, or `null` once it is answered, dismissed or
    /// taken back by the browser.
    get_question: GetQuestionArgs {
        /// The question.
        id: QuestionId,
    } -> Option<Question>;
    /// One download, for its progress window.
    get_download: GetDownloadArgs {
        /// The download.
        id: DownloadId,
    } -> DownloadSummary;
    /// The download in the list that a question is about again, if any.
    earlier_download: EarlierDownloadArgs {
        /// The question.
        id: QuestionId,
    } -> Option<DownloadSummary>;
    /// Brings every open question's window to the front, opening closed ones again.
    show_question_windows -> ();
    /// Opens (or brings forward) the progress window of a download.
    show_progress_window: ShowProgressWindowArgs {
        /// The download.
        id: DownloadId,
    } -> ();
    /// Closes the download window that calls it.
    close_download_window -> ();
    /// Makes the download window that calls it as tall as its content, within its screen.
    fit_download_window: FitDownloadWindowArgs {
        /// The content's height, in logical pixels.
        height: f64,
    } -> ();
    /// Shows the main window once its first view is drawn; it opens hidden, so it never
    /// flashes blank.
    main_window_ready -> ();
    /// Minimizes, maximizes or closes the window that calls it, for its caption buttons.
    control_window: ControlWindowArgs {
        /// What to do.
        action: WindowAction,
    } -> ();
    /// The state of the window that calls it; later changes arrive on `WINDOW_FRAME_EVENT`.
    window_frame -> WindowFrame;
    /// Says where the calling window's maximize button is, so that on Windows 11 hovering it offers the system's snap layouts; `null` when it has none. Call again whenever it moves.
    set_maximize_button: SetMaximizeButtonArgs {
        /// The button's area, or `null`.
        area: Option<ButtonArea>,
    } -> ();
    /// The system's accent colour; later changes arrive on `SYSTEM_APPEARANCE_EVENT`.
    system_appearance -> SystemAppearance;
    /// What the person should know about how Wirecatch started this time (a damaged download list set aside, downloads hidden from the list, a data folder other accounts may read); empty on a normal start. The main window shows each, so none is lost with the system notification.
    startup_notices -> Vec<StartupNotice>;
    /// Shows the download list set aside at this start (`StartupNotice::set_aside`) selected in its folder. The shell knows the path; the page sends none, so no page can open a path of its choosing.
    show_set_aside_list -> ();
    /// Browser downloads that started without a window of their own while the main window's page was not listening yet (it was opening or loading), for their notes; handed over once. From then on they arrive as `Captured` events (D36.7).
    take_captured -> Vec<DownloadSummary>;
    /// Logs that a window could not read an event the app sent it, and reloads everything instead; sent once per kind of event, so a mismatch between the two sides is found in the log rather than in a window showing wrong numbers.
    report_unreadable_event: ReportUnreadableEventArgs {
        /// The event's `type`, or `None` when it has none a window can read.
        kind: Option<String>,
    } -> ();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_is_named_once() {
        let mut names = COMMAND_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMAND_NAMES.len());
        assert_eq!(commands::add_download::NAME, CommandName("add_download"));
    }

    /// The interface sends each argument under its Rust name, inside `args`, so nothing relies
    /// on Tauri turning `camelCase` into `snake_case`.
    #[test]
    fn arguments_keep_their_rust_names() {
        let args: commands::reorder::Args =
            serde_json::from_str(r#"{"queue": 1, "ids_in_order": [3, 2]}"#).unwrap();
        assert_eq!(
            args,
            ReorderArgs {
                queue: QueueId(1),
                ids_in_order: vec![DownloadId(3), DownloadId(2)],
            }
        );
    }
}
