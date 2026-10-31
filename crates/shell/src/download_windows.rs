//! Which download windows are open and where they go (D23): one window per question the core
//! asks about a browser download, at most [`MAX_QUESTION_WINDOWS`] asking at once, each placed
//! one cascade step down and right of the last, as tall as its content. Pure bookkeeping, for
//! every interface that shows download windows.

use std::collections::{BTreeSet, VecDeque};

use wirecatch_core::Manager;
use wirecatch_engine::numeric::{f64_to_u64, saturating_cast};
use wirecatch_protocol::app::{
    AddonRequestItem, DOWNLOAD_WINDOW_CASCADE_STEP, DOWNLOAD_WINDOW_MAX_HEIGHT,
    DOWNLOAD_WINDOW_MIN_HEIGHT, DOWNLOAD_WINDOW_WIDTH, FileExistsQuestion, MAX_QUESTION_WINDOWS,
    MediaChoice, MediaReading, PendingCapture,
};
use wirecatch_protocol::{DownloadId, QuestionId};

/// Every download window's label starts with this. The capability in
/// `capabilities/download-windows.json` matches it as `download-*`.
pub const LABEL_PREFIX: &str = "download-";

/// A download window's size when it opens, in logical pixels.
pub const WINDOW_SIZE: (f64, f64) = (DOWNLOAD_WINDOW_WIDTH as f64, 600.0);

/// The smallest a download window can be made, in logical pixels.
pub const MIN_WINDOW_SIZE: (f64, f64) = (360.0, DOWNLOAD_WINDOW_MIN_HEIGHT as f64);

/// The most a download window takes, in logical pixels: past it, its body scrolls.
pub const MAX_WINDOW_HEIGHT: f64 = DOWNLOAD_WINDOW_MAX_HEIGHT as f64;

/// The space kept free above and below a window that fits its content, in logical pixels.
pub const SCREEN_MARGIN: f64 = 48.0;

/// The height a download window takes for content `wanted` pixels tall: what it needs, never
/// less than the smallest window and never taller than the most a window takes or the screen it
/// is on (`screen`, the height of its work area, when the system says). A height that is not a number gives the
/// smallest window.
#[must_use]
pub fn fitted_height(wanted: f64, screen: Option<f64>) -> f64 {
    let floor = MIN_WINDOW_SIZE.1;
    let ceiling = screen.filter(|h| h.is_finite()).map_or(WINDOW_SIZE.1, |h| {
        (h - 2.0 * SCREEN_MARGIN).min(MAX_WINDOW_HEIGHT).max(floor)
    });
    if !wanted.is_finite() {
        return floor;
    }
    wanted.ceil().clamp(floor, ceiling)
}

/// Whether `label` is a download window's.
#[must_use]
pub fn is_download_window(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// What a download window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shows {
    /// An open question, by its id in the core.
    Question(QuestionId),
    /// A download's progress.
    Download(DownloadId),
}

impl Shows {
    /// The label of a window opened for this.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Question(id) => format!("{LABEL_PREFIX}q{}", id.0),
            Self::Download(id) => format!("{LABEL_PREFIX}d{}", id.0),
        }
    }
}

/// An open download window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWindow {
    /// Its label.
    pub label: String,
    /// What it shows now.
    pub shows: Shows,
    /// Its place in the cascade (see [`place`]).
    pub slot: usize,
}

/// What to do with the windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Create this window.
    Open(OpenWindow),
    /// Bring the window with this label to the front.
    Focus(String),
}

/// Which download windows are open, which questions wait for a window, and which ones the
/// person put aside by closing their window. Pure bookkeeping: [`Step`]s say what to do.
#[derive(Debug, Default)]
pub struct Registry {
    windows: Vec<OpenWindow>,
    waiting: VecDeque<QuestionId>,
    put_aside: BTreeSet<QuestionId>,
}

impl Registry {
    /// The open windows, in the order they opened.
    #[must_use]
    pub fn windows(&self) -> &[OpenWindow] {
        &self.windows
    }

    /// What the window `label` shows now, or `None` when no such window is open.
    #[must_use]
    pub fn shows(&self, label: &str) -> Option<Shows> {
        self.windows
            .iter()
            .find(|w| w.label == label)
            .map(|w| w.shows)
    }

    /// Questions waiting for room to open their window, oldest first.
    pub fn waiting(&self) -> impl Iterator<Item = QuestionId> + '_ {
        self.waiting.iter().copied()
    }

    fn asking(&self) -> usize {
        self.windows
            .iter()
            .filter(|w| matches!(w.shows, Shows::Question(_)))
            .count()
    }

    /// The lowest slot no open window takes. One of the first `windows.len() + 1` slots is
    /// always free, so the search ends there.
    fn free_slot(&self) -> usize {
        (0..=self.windows.len())
            .find(|slot| !self.windows.iter().any(|w| w.slot == *slot))
            .unwrap_or(self.windows.len())
    }

    #[must_use]
    pub fn window_showing(&self, shows: Shows) -> Option<&OpenWindow> {
        self.windows.iter().find(|w| w.shows == shows)
    }

    fn place_question(&mut self, id: QuestionId) -> Option<Step> {
        if self.window_showing(Shows::Question(id)).is_some() || self.waiting.contains(&id) {
            return None;
        }
        if self.asking() >= MAX_QUESTION_WINDOWS {
            self.waiting.push_back(id);
            return None;
        }
        let shows = Shows::Question(id);
        let window = OpenWindow {
            label: shows.label(),
            shows,
            slot: self.free_slot(),
        };
        self.windows.push(window.clone());
        Some(Step::Open(window))
    }

    /// The core asked question `id`, or moved it on (the video was read, the add-on is
    /// needed): its window opens, or waits for room, unless it is already open or waiting.
    pub fn ask(&mut self, id: QuestionId) -> Option<Step> {
        self.put_aside.remove(&id);
        self.place_question(id)
    }

    /// Questions found open in the core (`open`, oldest first) at start or after missed
    /// events: windows for those that have none, except the ones the person put aside.
    pub fn restore(&mut self, open: &[QuestionId]) -> Vec<Step> {
        self.put_aside.retain(|id| open.contains(id));
        self.waiting.retain(|id| open.contains(id));
        let mut steps = Vec::new();
        for id in open {
            if !self.put_aside.contains(id) {
                steps.extend(self.place_question(*id));
            }
        }
        steps
    }

    /// The person asked to see every open question (`open`, oldest first): open windows come
    /// to the front, closed ones open again.
    pub fn show_all(&mut self, open: &[QuestionId]) -> Vec<Step> {
        self.waiting.retain(|id| open.contains(id));
        let mut steps = Vec::new();
        for id in open {
            self.put_aside.remove(id);
            match self.window_showing(Shows::Question(*id)) {
                Some(window) => steps.push(Step::Focus(window.label.clone())),
                None => steps.extend(self.place_question(*id)),
            }
        }
        steps
    }

    /// The person asked for question `id` again (the browser sent its link again): its window
    /// comes to the front, or opens again even if they had put it aside (D89).
    pub fn show_question(&mut self, id: QuestionId) -> Option<Step> {
        self.put_aside.remove(&id);
        match self.window_showing(Shows::Question(id)) {
            Some(window) => Some(Step::Focus(window.label.clone())),
            None => self.place_question(id),
        }
    }

    /// Question `id` is no longer open in the core. Its window, if any, says so itself.
    pub fn closed(&mut self, id: QuestionId) {
        self.waiting.retain(|w| *w != id);
        self.put_aside.remove(&id);
    }

    /// The window `label` now shows download `id` (its question was answered there): it no
    /// longer counts as asking, so a waiting question may open.
    pub fn became_download(&mut self, label: &str, id: DownloadId) -> Vec<Step> {
        if let Some(window) = self.windows.iter_mut().find(|w| w.label == label) {
            window.shows = Shows::Download(id);
        }
        self.fill()
    }

    /// The window `label` is gone. A question it showed was put aside by the person (closing
    /// is not an answer), so it does not open again by itself. Waiting questions may open.
    pub fn gone(&mut self, label: &str) -> Vec<Step> {
        if let Some(at) = self.windows.iter().position(|w| w.label == label) {
            let window = self.windows.remove(at);
            if let Shows::Question(id) = window.shows {
                self.put_aside.insert(id);
            }
        }
        self.fill()
    }

    /// The window `label` could not be created: forget it, without putting its question aside.
    pub fn failed(&mut self, label: &str) -> Vec<Step> {
        self.windows.retain(|w| w.label != label);
        self.fill()
    }

    /// "Open in its own window" for download `id`: the window already showing it comes to the
    /// front, or a new one opens. Not limited: the person asked for it.
    pub fn show_download(&mut self, id: DownloadId) -> Step {
        if let Some(window) = self.window_showing(Shows::Download(id)) {
            return Step::Focus(window.label.clone());
        }
        let shows = Shows::Download(id);
        let window = OpenWindow {
            label: shows.label(),
            shows,
            slot: self.free_slot(),
        };
        self.windows.push(window.clone());
        Step::Open(window)
    }

    fn fill(&mut self) -> Vec<Step> {
        let mut steps = Vec::new();
        while self.asking() < MAX_QUESTION_WINDOWS {
            let Some(id) = self.waiting.pop_front() else {
                break;
            };
            steps.extend(self.place_question(id));
        }
        steps
    }
}

/// A screen's usable area, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

/// Where the window in `slot` goes on `area`, for a window of `size` (all logical pixels). The
/// first windows spread around the middle of the screen, each [`DOWNLOAD_WINDOW_CASCADE_STEP`]
/// down and right of the one before; slots that would leave the screen start again at the top
/// left of the cascade. A window larger than the screen goes to its top left corner.
#[must_use]
pub fn place(slot: usize, area: Area, size: (f64, f64)) -> (f64, f64) {
    let room_x = (area.width - size.0).max(0.0);
    let room_y = (area.height - size.1).max(0.0);
    // The first few windows centre on the screen together.
    let lead = cascade_offset(MAX_QUESTION_WINDOWS - 1) / 2.0;
    let start_x = (room_x / 2.0 - lead).max(0.0);
    let start_y = (room_y / 2.0 - lead).max(0.0);
    let fit = whole_steps(room_x - start_x)
        .min(whole_steps(room_y - start_y))
        .saturating_add(1);
    let offset = cascade_offset(slot % fit);
    (area.x + start_x + offset, area.y + start_y + offset)
}

/// `steps` cascade steps, in logical pixels: whole numbers, so exact as an `f64`. A count no
/// screen could hold gives no offset; [`place`] never asks for one.
fn cascade_offset(steps: usize) -> f64 {
    u32::try_from(steps)
        .ok()
        .and_then(|n| n.checked_mul(DOWNLOAD_WINDOW_CASCADE_STEP))
        .map_or(0.0, f64::from)
}

/// How many whole cascade steps fit in `length` logical pixels: none when it is negative or
/// not a number.
fn whole_steps(length: f64) -> usize {
    f64_to_u64((length / f64::from(DOWNLOAD_WINDOW_CASCADE_STEP)).floor())
        .map_or(0, saturating_cast)
}

/// Every open question's id, oldest first (ids only grow), each once.
#[must_use]
pub fn open_question_ids(
    captures: &[PendingCapture],
    choices: &[MediaChoice],
    addon_pages: &[AddonRequestItem],
    reading: &[MediaReading],
    files: &[FileExistsQuestion],
) -> Vec<QuestionId> {
    let mut ids: Vec<QuestionId> = captures
        .iter()
        .map(|c| c.id)
        .chain(choices.iter().map(|c| c.id))
        .chain(addon_pages.iter().map(|a| a.id))
        .chain(reading.iter().map(|r| r.id))
        .chain(files.iter().map(|f| f.id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Every question open in the core right now, oldest first.
#[must_use]
pub fn open_questions(manager: &Manager) -> Vec<QuestionId> {
    open_question_ids(
        &manager.pending_confirmations(),
        &manager.pending_media_choices(),
        &manager.pending_addon_requests(),
        &manager.media_being_read(),
        &manager.pending_file_exists(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(id: u64) -> QuestionId {
        QuestionId(id)
    }

    #[test]
    fn a_window_takes_the_height_its_content_needs_within_the_screen() {
        // A short progress view: smaller than the opening size, rounded up to whole pixels.
        assert_eq!(fitted_height(431.2, Some(1040.0)), 432.0);
        // Never smaller than the smallest window.
        assert_eq!(fitted_height(120.0, Some(1040.0)), MIN_WINDOW_SIZE.1);
        // A long quality list, or a group opened: never taller than the most a window takes;
        // the body scrolls.
        assert_eq!(fitted_height(2000.0, Some(1440.0)), MAX_WINDOW_HEIGHT);
        // On a short screen, as tall as the screen allows, with a margin.
        assert_eq!(
            fitted_height(2000.0, Some(700.0)),
            700.0 - 2.0 * SCREEN_MARGIN
        );
        // Unknown screen: no taller than the opening size.
        assert_eq!(fitted_height(900.0, None), WINDOW_SIZE.1);
        // A tiny screen still gets the smallest window, and nonsense gives the smallest too.
        assert_eq!(fitted_height(700.0, Some(300.0)), MIN_WINDOW_SIZE.1);
        assert_eq!(fitted_height(f64::NAN, Some(1040.0)), MIN_WINDOW_SIZE.1);
        assert_eq!(
            fitted_height(f64::INFINITY, Some(1040.0)),
            MIN_WINDOW_SIZE.1
        );
    }

    fn opened(step: Option<Step>) -> OpenWindow {
        match step {
            Some(Step::Open(window)) => window,
            other => panic!("expected a window to open, got {other:?}"),
        }
    }

    #[test]
    fn a_window_is_known_by_what_it_shows_now() {
        let mut registry = Registry::default();
        let window = opened(registry.ask(q(4)));
        assert_eq!(registry.shows(&window.label), Some(Shows::Question(q(4))));
        registry.became_download(&window.label, DownloadId(9));
        assert_eq!(
            registry.shows(&window.label),
            Some(Shows::Download(DownloadId(9)))
        );
        registry.gone(&window.label);
        assert_eq!(registry.shows(&window.label), None);
        assert_eq!(registry.shows("main"), None);
    }

    #[test]
    fn at_most_five_questions_ask_at_once_and_the_rest_wait_their_turn() {
        let mut r = Registry::default();
        let slots: Vec<usize> = (1..=5).map(|id| opened(r.ask(q(id))).slot).collect();
        assert_eq!(slots, [0, 1, 2, 3, 4]);
        assert_eq!(r.ask(q(6)), None);
        assert_eq!(r.ask(q(7)), None);
        assert_eq!(r.waiting().collect::<Vec<_>>(), [q(6), q(7)]);
        // Asking again (the video was read) opens nothing twice.
        assert_eq!(r.ask(q(3)), None);
        assert_eq!(r.ask(q(6)), None);

        // A window that starts showing its download makes room; it keeps its place.
        let steps = r.became_download("download-q2", DownloadId(40));
        let [Step::Open(next)] = steps.as_slice() else {
            panic!("{steps:?}")
        };
        assert_eq!((next.shows, next.slot), (Shows::Question(q(6)), 5));
        assert!(r.windows().iter().any(|w| w.label == "download-q2"
            && w.shows == Shows::Download(DownloadId(40))
            && w.slot == 1));

        // A closed window makes room too, and its slot is used again.
        let steps = r.gone("download-q1");
        let [Step::Open(next)] = steps.as_slice() else {
            panic!("{steps:?}")
        };
        assert_eq!((next.shows, next.slot), (Shows::Question(q(7)), 0));
    }

    #[test]
    fn a_question_closed_while_waiting_never_opens() {
        let mut r = Registry::default();
        for id in 1..=6 {
            r.ask(q(id));
        }
        r.closed(q(6));
        assert_eq!(r.gone("download-q1"), []);
    }

    #[test]
    fn a_window_closed_by_the_person_stays_closed_until_asked_for() {
        let mut r = Registry::default();
        opened(r.ask(q(1)));
        opened(r.ask(q(2)));
        assert_eq!(r.gone("download-q1"), []);
        // At start or after missed events, a put-aside question does not pop up again, and
        // one still open is not opened twice.
        assert_eq!(r.restore(&[q(1), q(2)]), []);
        // The person asks for the waiting questions: the open one comes forward, the closed
        // one opens again.
        let steps = r.show_all(&[q(1), q(2)]);
        assert_eq!(steps.len(), 2);
        assert!(steps.contains(&Step::Focus("download-q2".into())));
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, Step::Open(w) if w.shows == Shows::Question(q(1))))
        );
        // The core moving a put-aside question on (its quality list is ready) opens it.
        assert_eq!(r.gone("download-q1"), []);
        opened(r.ask(q(1)));
    }

    /// The browser sent the link again (D89): an open window comes forward, a closed one opens.
    #[test]
    fn a_link_sent_again_shows_its_question() {
        let mut r = Registry::default();
        opened(r.ask(q(1)));
        assert_eq!(
            r.show_question(q(1)),
            Some(Step::Focus("download-q1".into()))
        );
        assert_eq!(r.gone("download-q1"), []);
        assert!(matches!(
            r.show_question(q(1)),
            Some(Step::Open(w)) if w.shows == Shows::Question(q(1))
        ));
    }

    #[test]
    fn restore_opens_only_what_is_open_in_the_core_and_has_no_window() {
        let mut r = Registry::default();
        opened(r.ask(q(3)));
        let steps = r.restore(&[q(2), q(3), q(4)]);
        let shown: Vec<Shows> = steps
            .iter()
            .map(|s| match s {
                Step::Open(w) => w.shows,
                Step::Focus(l) => panic!("focused {l}"),
            })
            .collect();
        assert_eq!(shown, [Shows::Question(q(2)), Shows::Question(q(4))]);
        // Answered questions are not in the core's lists, so nothing opens for them.
        assert_eq!(r.restore(&[q(2), q(3), q(4)]), []);
    }

    #[test]
    fn a_progress_window_is_opened_once_and_is_not_limited() {
        let mut r = Registry::default();
        for id in 1..=5 {
            r.ask(q(id));
        }
        let Step::Open(window) = r.show_download(DownloadId(9)) else {
            panic!("expected a new window")
        };
        assert_eq!(window.label, "download-d9");
        assert_eq!(window.slot, 5);
        assert_eq!(
            r.show_download(DownloadId(9)),
            Step::Focus("download-d9".into())
        );
        // A question window that became download 9's is the one shown.
        r.became_download("download-q1", DownloadId(10));
        assert_eq!(
            r.show_download(DownloadId(10)),
            Step::Focus("download-q1".into())
        );
        // A window that could not be made is forgotten without being put aside.
        r.failed("download-q2");
        assert_eq!(r.restore(&[q(2)]).len(), 1);
    }

    const SCREEN: Area = Area {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };

    #[test]
    fn each_window_sits_one_step_down_and_right_of_the_last() {
        let first = place(0, SCREEN, WINDOW_SIZE);
        let second = place(1, SCREEN, WINDOW_SIZE);
        let step = f64::from(DOWNLOAD_WINDOW_CASCADE_STEP);
        assert_eq!(second.0 - first.0, step);
        assert_eq!(second.1 - first.1, step);
        // The first five spread around the middle of the screen.
        let fifth = place(4, SCREEN, WINDOW_SIZE);
        let middle = (
            (first.0 + fifth.0 + WINDOW_SIZE.0) / 2.0,
            (first.1 + fifth.1 + WINDOW_SIZE.1) / 2.0,
        );
        assert_eq!(middle, (960.0, 520.0));
    }

    #[test]
    fn windows_stay_on_their_screen() {
        let second_screen = Area {
            x: 1920.0,
            y: -200.0,
            width: 1280.0,
            height: 680.0,
        };
        for slot in 0..40 {
            let (x, y) = place(slot, second_screen, WINDOW_SIZE);
            assert!(x >= second_screen.x && y >= second_screen.y, "slot {slot}");
            assert!(
                x + WINDOW_SIZE.0 <= second_screen.x + second_screen.width,
                "slot {slot}"
            );
            assert!(
                y + WINDOW_SIZE.1 <= second_screen.y + second_screen.height,
                "slot {slot}"
            );
        }
        // Slots past the room start the cascade again.
        assert_eq!(
            place(0, second_screen, WINDOW_SIZE),
            place(3, second_screen, WINDOW_SIZE)
        );
        // A screen smaller than the window: its top left corner.
        let tiny = Area {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 300.0,
        };
        assert_eq!(place(0, tiny, WINDOW_SIZE), (10.0, 20.0));
        assert_eq!(place(7, tiny, WINDOW_SIZE), (10.0, 20.0));
    }
}
