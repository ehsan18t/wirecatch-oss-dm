// Generated from config/product.toml by `pnpm product:generate`. Do not edit: change config/product.toml and run it again.

//! Wirecatch's product identity: names, ids, executables, folders and addresses, written once in `config/product.toml` (D29).
//!
//! Every binary that needs one of these values takes it from here, so a rename changes one file. Values that are not decided yet hold a placeholder (see `config/product.toml`), which the release check refuses.

/// The product name people see.
pub const PRODUCT_NAME: &str = "Wirecatch";

/// The release version of the app and the extension.
pub const PRODUCT_VERSION: &str = "0.1.0";

/// The desktop app executable, without `.exe`.
pub const APP_EXECUTABLE: &str = "wirecatch-app";

/// The browser bridge executable, without `.exe`; it lives next to the app (D19).
pub const BRIDGE_EXECUTABLE: &str = "wirecatch-bridge";

/// The command-line downloader executable, without `.exe`.
pub const CLI_EXECUTABLE: &str = "wirecatch";

/// The download list, in the data folder.
pub const DATABASE_FILE: &str = "wirecatch.db";

/// The native messaging host name the bridge registers and the extension connects to.
pub const NATIVE_HOST_NAME: &str = "app.wirecatch.bridge";

/// The description in the bridge's native messaging manifests.
pub const NATIVE_HOST_DESCRIPTION: &str = "Wirecatch browser bridge";

/// The one Chromium extension id every bridge trusts (D22).
pub const CHROMIUM_EXTENSION_ID: &str = "chdgnmpjoipbgfeldicipccjjghmagki";

/// The origin Chromium passes for the extension, in the exact form it uses.
pub const CHROMIUM_EXTENSION_ORIGIN: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";

/// The Firefox add-on id.
pub const FIREFOX_EXTENSION_ID: &str = "extension@wirecatch.app";

/// Linux: the folder in `XDG_RUNTIME_DIR` that holds the app's socket.
pub const IPC_RUNTIME_FOLDER: &str = "wirecatch";

/// Linux: the start of the folder in `/tmp` that holds the socket when there is no runtime folder; the user id follows it.
pub const IPC_FALLBACK_FOLDER_PREFIX: &str = "wirecatch-";

/// [`APP_EXECUTABLE`] as a file name on this system.
pub const APP_FILE_NAME: &str = if cfg!(windows) {
    "wirecatch-app.exe"
} else {
    APP_EXECUTABLE
};

/// [`BRIDGE_EXECUTABLE`] as a file name on this system.
pub const BRIDGE_FILE_NAME: &str = if cfg!(windows) {
    "wirecatch-bridge.exe"
} else {
    BRIDGE_EXECUTABLE
};

/// A build of the app that runs as an app of its own, next to the others, without touching their downloads, endpoint or settings (D13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Channel {
    /// Release builds.
    Release,
    /// Debug builds.
    Dev,
}

impl Channel {
    /// The name this channel shows, which is also its start-at-sign-in entry.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Release => "Wirecatch",
            Self::Dev => "Wirecatch Dev",
        }
    }

    /// The app identifier, which keeps the channels apart for the operating system and the single-instance guard.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::Release => "app.wirecatch.desktop",
            Self::Dev => "app.wirecatch.desktop.dev",
        }
    }

    /// The data folder's name in `%LOCALAPPDATA%` on Windows.
    #[must_use]
    pub const fn windows_data_folder(self) -> &'static str {
        match self {
            Self::Release => "Wirecatch",
            Self::Dev => "Wirecatch Dev",
        }
    }

    /// The data folder's name in `~/.local/share` on Linux.
    #[must_use]
    pub const fn linux_data_folder(self) -> &'static str {
        match self {
            Self::Release => "wirecatch",
            Self::Dev => "wirecatch-dev",
        }
    }

    /// Windows: the pipe name before the user's SID, after `\\.\pipe\`.
    #[must_use]
    pub const fn pipe_prefix(self) -> &'static str {
        match self {
            Self::Release => "wirecatch-app-",
            Self::Dev => "wirecatch-app-dev-",
        }
    }

    /// Linux: the socket's file name in the runtime folder.
    #[must_use]
    pub const fn socket_name(self) -> &'static str {
        match self {
            Self::Release => "app.sock",
            Self::Dev => "app-dev.sock",
        }
    }

    /// The data folder's name on this system: [`Self::windows_data_folder`] on Windows, [`Self::linux_data_folder`] elsewhere.
    #[must_use]
    pub const fn data_folder(self) -> &'static str {
        if cfg!(windows) {
            self.windows_data_folder()
        } else {
            self.linux_data_folder()
        }
    }
}
