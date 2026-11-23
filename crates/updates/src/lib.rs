//! Updates of the Wirecatch app (D111, D288): GitHub's release list and asset digests, the signed
//! list of a release's installers, which release is newer, and which installer suits this
//! computer. Pure: fetching and installing are the caller's.

mod installer_name;
mod manifest;
mod release;

pub use installer_name::installer_name;
pub use manifest::{Arch, FileKind, Manifest, ManifestError, System, UpdateFile};
pub use release::{Asset, Release, newest};
pub use semver::Version;
