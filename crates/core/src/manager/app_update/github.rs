use wirecatch_engine::Url;
use wirecatch_updates::{Arch, FileKind, Release, System, UpdateFile, Version};

use super::UpdateDownload;

/// The normal release URL or its asset's download URL, under the configured repository.
pub(super) fn release_url(repository: &Url, tag: &str, asset: Option<&str>) -> Option<Url> {
    let mut url = repository.clone();
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty().push("releases");
        if let Some(asset) = asset {
            segments.extend(["download", tag, asset]);
        } else {
            segments.extend(["tag", tag]);
        }
    }
    Some(url)
}

pub(super) fn installer_name(
    version: &Version,
    system: System,
    arch: Arch,
    kind: FileKind,
) -> Option<String> {
    let system = match system {
        System::Windows => "windows",
        System::Linux => "linux",
    };
    let arch = match arch {
        Arch::X64 => "x64",
        Arch::Arm64 => "arm64",
    };
    let kind = match kind {
        FileKind::Setup => "setup",
        FileKind::AppImage => "appimage",
        FileKind::Deb => "deb",
        FileKind::Rpm => "rpm",
    };
    let suffix = wirecatch_product::installer_suffix(system, arch, kind)?;
    Some(format!(
        "{}-{version}-{suffix}",
        wirecatch_product::PRODUCT_NAME
    ))
}

pub(super) fn installer(
    repository: &Url,
    release: &Release,
    version: &Version,
    kind: FileKind,
) -> Option<UpdateDownload> {
    let (system, arch) = (System::current()?, Arch::current()?);
    let name = installer_name(version, system, arch, kind)?;
    let mut matches = release.assets.iter().filter(|asset| asset.name == name);
    let asset = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let sha256 = asset.uploaded_sha256()?.to_owned();
    let url = release_url(repository, &release.tag_name, Some(&name))?;
    Some(UpdateDownload {
        url,
        file: UpdateFile {
            name,
            system,
            arch,
            kind,
            size: asset.size,
            sha256,
        },
    })
}
