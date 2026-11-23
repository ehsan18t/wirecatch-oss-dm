// Generated from scripts/release/installers.mjs by `pnpm product:generate`. Do not edit: change that file and run it again.

use crate::{Arch, FileKind, System, Version};

/// The filename of an installer published for this system, architecture and kind.
#[must_use]
pub fn installer_name(
    prefix: &str,
    version: &Version,
    system: System,
    arch: Arch,
    kind: FileKind,
) -> Option<String> {
    let suffix = match (system, arch, kind) {
        (System::Windows, Arch::X64, FileKind::Setup) => "windows-x64-setup.exe",
        (System::Windows, Arch::Arm64, FileKind::Setup) => "windows-arm64-setup.exe",
        (System::Linux, Arch::X64, FileKind::AppImage) => "linux-x64.AppImage",
        (System::Linux, Arch::Arm64, FileKind::AppImage) => "linux-arm64.AppImage",
        (System::Linux, Arch::X64, FileKind::Deb) => "linux-x64.deb",
        (System::Linux, Arch::Arm64, FileKind::Deb) => "linux-arm64.deb",
        (System::Linux, Arch::X64, FileKind::Rpm) => "linux-x64.rpm",
        (System::Linux, Arch::Arm64, FileKind::Rpm) => "linux-arm64.rpm",
        _ => return None,
    };
    Some(format!("{prefix}-{version}-{suffix}"))
}
