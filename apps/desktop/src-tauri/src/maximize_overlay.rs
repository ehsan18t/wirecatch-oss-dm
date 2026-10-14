//! The invisible window the shell lays over a page's maximize button, so Windows 11 offers its
//! snap layouts there (see [`crate::frame`]). Its window procedure answers every hit test with
//! `HTMAXBUTTON`. It never paints, so the page's own button shows through it, and it carries the
//! button's name ("Maximize" or "Restore") for screen readers that explore by pointer.
//!
//! The only unsafe code of the title bars, behind [`place`]: every overlay is created, moved,
//! renamed and destroyed on the window thread, where its procedure also runs, and the handles
//! are kept per window thread, never shared.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;

use tauri::{Manager as _, Runtime, WebviewWindow};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetParent, GetWindowLongPtrW,
    HTMAXBUTTON, HWND_TOP, IsWindow, RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, WM_NCDESTROY, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_VISIBLE,
};

use crate::frame::PhysicalArea;

/// The overlay's style: a child of its window, so it is never a window of its own in the
/// taskbar, Alt+Tab or the minimized windows, and never takes the activation.
const STYLE: u32 = WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS;
/// Its extended style: a tool window that is never activated, should it ever stand alone.
const EX_STYLE: u32 = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;

/// One overlay: its handle and its parent's, kept as numbers (a handle is not `Send`, and these
/// never leave the window thread).
#[derive(Clone, Copy)]
struct Overlay {
    hwnd: isize,
    parent: isize,
}

thread_local! {
    /// The overlays on this thread's windows, by window label.
    static OVERLAYS: RefCell<HashMap<String, Overlay>> = RefCell::new(HashMap::new());
}

/// What the overlay tells the shell.
enum Pointer {
    Over(bool),
    Clicked,
}

/// What the window procedure needs, owned by the overlay window and freed with it.
struct Context {
    /// The pointer is over the overlay and the system will say when it leaves.
    tracking: Cell<bool>,
    /// The button was pressed here and not let go yet: only then does letting go click it.
    pressed: Cell<bool>,
    on_pointer: Box<dyn Fn(Pointer)>,
}

/// `text` as the system's null-terminated wide strings.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn class_name() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| wide("WirecatchMaximizeButton"))
}

/// Registers the overlay's window class once; `false` if the system refused.
fn registered() -> bool {
    static ATOM: OnceLock<u16> = OnceLock::new();
    *ATOM.get_or_init(|| {
        let class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(procedure),
            cbClsExtra: 0,
            cbWndExtra: 0,
            // SAFETY: a null name asks for this program's own module, which always exists.
            hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            // No brush: the window never paints, so the page shows through.
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name().as_ptr(),
        };
        // SAFETY: `class` is fully initialised and its strings are 'static.
        unsafe { RegisterClassW(&raw const class) }
    }) != 0
}

impl Overlay {
    /// Whether the handle still names our overlay on `parent`. A child window is also destroyed
    /// with its parent, and the system may then give its handle to another window.
    fn alive_on(self, parent: isize) -> bool {
        let hwnd = self.hwnd as HWND;
        // SAFETY: both calls only read a handle's state, and fail cleanly on a stale one.
        let ours = unsafe { IsWindow(hwnd) != 0 && GetParent(hwnd) as isize == parent };
        ours && self.parent == parent
    }
}

/// Puts the overlay of `window` over `area` (physical pixels), named for maximizing or, with
/// `restores`, restoring; creates it the first time, removes it for `None`. Runs on the window
/// thread.
pub fn place<R: Runtime>(window: &WebviewWindow<R>, area: Option<PhysicalArea>, restores: bool) {
    let label = window.label().to_owned();
    let Ok(parent) = window.hwnd().map(|h| h.0 as isize) else {
        return;
    };
    let existing = OVERLAYS
        .with_borrow_mut(|overlays| overlays.remove(&label))
        .filter(|overlay| overlay.alive_on(parent));
    let Some((x, y, width, height)) = area else {
        if let Some(overlay) = existing {
            // SAFETY: checked above to be our live overlay; destroying it frees its context
            // (WM_NCDESTROY).
            unsafe { DestroyWindow(overlay.hwnd as HWND) };
        }
        crate::frame::hover(window.app_handle(), &label, false);
        return;
    };
    let Some(overlay) = existing.or_else(|| create(window, parent)) else {
        return;
    };
    let name = wide(if restores { "Restore" } else { "Maximize" });
    let hwnd = overlay.hwnd as HWND;
    // SAFETY: `hwnd` is our live overlay (checked above, or just created), and `name` is
    // null-terminated and outlives the call.
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOP,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        SetWindowTextW(hwnd, name.as_ptr());
    }
    OVERLAYS.with_borrow_mut(|overlays| overlays.insert(label, overlay));
}

fn create<R: Runtime>(window: &WebviewWindow<R>, parent: isize) -> Option<Overlay> {
    if !registered() {
        tracing::warn!("could not register the maximize button's window class");
        return None;
    }
    let app = window.app_handle().clone();
    let label = window.label().to_owned();
    let context = Box::new(Context {
        tracking: Cell::new(false),
        pressed: Cell::new(false),
        on_pointer: Box::new(move |pointer| match pointer {
            Pointer::Over(over) => crate::frame::hover(&app, &label, over),
            Pointer::Clicked => {
                if let Some(window) = app.get_webview_window(&label) {
                    tauri::async_runtime::spawn(async move {
                        crate::window::warn_if_failed(
                            "maximize or restore the window",
                            crate::frame::toggle_maximize(&window),
                        );
                    });
                }
            }
        }),
    });
    // SAFETY: the class is registered, `parent` is the live window this runs for, and the
    // strings are 'static and null-terminated.
    let hwnd = unsafe {
        CreateWindowExW(
            EX_STYLE,
            class_name().as_ptr(),
            std::ptr::null(),
            STYLE,
            0,
            0,
            0,
            0,
            parent as HWND,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        tracing::warn!("could not create the window over the maximize button");
        return None;
    }
    // SAFETY: `hwnd` was just created; the context it now owns is freed on WM_NCDESTROY.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(context) as isize);
    }
    Some(Overlay {
        hwnd: hwnd as isize,
        parent,
    })
}

/// The overlay's window procedure.
unsafe extern "system" fn procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: the user data is null until `create` stores the context, and points to it until
    // WM_NCDESTROY takes it back below; messages arrive on the window thread only.
    let context = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Context;
    if context.is_null() {
        // SAFETY: the default procedure for a message this window does not handle.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_NCDESTROY {
        // SAFETY: the pointer came from `Box::into_raw` in `create` and is taken back once,
        // here, after the user data is cleared so nothing reads it again.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(context.cast_mut()));
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
    }
    // SAFETY: checked non-null above, and alive until WM_NCDESTROY, handled above.
    let context = unsafe { &*context };
    match message {
        WM_NCHITTEST => return LRESULT::try_from(HTMAXBUTTON).unwrap_or_default(),
        WM_NCMOUSEMOVE => {
            if !context.tracking.replace(true) {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: u32::try_from(size_of::<TRACKMOUSEEVENT>()).unwrap_or(0),
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: `track` is initialised and names this live window.
                unsafe { TrackMouseEvent(&raw mut track) };
                (context.on_pointer)(Pointer::Over(true));
            }
            return 0;
        }
        WM_NCMOUSELEAVE => {
            context.tracking.set(false);
            context.pressed.set(false);
            (context.on_pointer)(Pointer::Over(false));
            return 0;
        }
        // The press is the system's cue to track a caption button of this window itself; as a
        // caption button does, the click needs the press and the release both on the button.
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            context.pressed.set(true);
            return 0;
        }
        WM_NCLBUTTONUP => {
            if context.pressed.replace(false) {
                (context.on_pointer)(Pointer::Clicked);
            }
            return 0;
        }
        _ => {}
    }
    // SAFETY: the default procedure for everything else.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_CAPTION, WS_EX_APPWINDOW, WS_POPUP};

    /// A stray Wirecatch window in the taskbar, Alt+Tab or the minimized windows would be a
    /// top-level one; the overlay is a child and a tool window, and never an app window.
    #[test]
    fn the_overlay_is_never_a_window_of_its_own() {
        assert_eq!(STYLE & WS_CHILD, WS_CHILD);
        assert_eq!(STYLE & (WS_POPUP | WS_CAPTION), 0);
        assert_eq!(EX_STYLE & WS_EX_TOOLWINDOW, WS_EX_TOOLWINDOW);
        assert_eq!(EX_STYLE & WS_EX_NOACTIVATE, WS_EX_NOACTIVATE);
        assert_eq!(EX_STYLE & WS_EX_APPWINDOW, 0);
    }
}
