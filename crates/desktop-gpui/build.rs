//! Gives the Windows executable its icon and version details, which Explorer, the taskbar and
//! Task Manager show. GPUI loads the window icon from resource 1. Other systems need nothing.

use std::error::Error;
use std::path::PathBuf;

use wirecatch_product::{APP_EXECUTABLE, Channel, PRODUCT_VERSION};

const ICON: &str = "../../apps/desktop/src-tauri/icons/icon.ico";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let channel = if std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some() {
        Channel::Dev
    } else {
        Channel::Release
    };
    let numbers: Vec<u16> = PRODUCT_VERSION
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let [major, minor, patch] = [0, 1, 2].map(|i| numbers.get(i).copied().unwrap_or(0));
    // The resource compiler reads backslashes as escapes.
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").ok_or("no manifest dir")?);
    let icon = manifest
        .join(ICON)
        .display()
        .to_string()
        .replace('\\', "\\\\");
    let name = channel.name();
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "{name}"
      VALUE "ProductName", "{name}"
      VALUE "FileVersion", "{PRODUCT_VERSION}"
      VALUE "ProductVersion", "{PRODUCT_VERSION}"
      VALUE "OriginalFilename", "{APP_EXECUTABLE}.exe"
      VALUE "InternalName", "{APP_EXECUTABLE}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let file =
        PathBuf::from(std::env::var_os("OUT_DIR").ok_or("cargo sets OUT_DIR")?).join("app.rc");
    std::fs::write(&file, rc)?;
    embed_resource::compile(&file, embed_resource::NONE).manifest_optional()?;
    Ok(())
}
