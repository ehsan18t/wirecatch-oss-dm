//! Updates of the Wirecatch app (D111): the signed list of a release's installers, which release
//! is newer, and which installer suits this computer. Pure: fetching and installing are the
//! caller's.

mod manifest;
mod release;

pub use manifest::{Arch, FileKind, Manifest, ManifestError, System, UpdateFile};
pub use release::{Asset, Release, newest};
pub use semver::Version;
