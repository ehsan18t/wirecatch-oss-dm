//! The stand-in for PhantomJS that yt-dlp finds on its `PATH` (D281): some extractors answer a site's check through yt-dlp's `PhantomJSwrapper`, and PhantomJS itself is long abandoned. The stand-in runs those scripts in the media add-on's Deno, allowed only into the run's own folder.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wirecatch_product::PRODUCT_SLUG;

/// The stand-in's program name: the one yt-dlp looks for.
pub const PROGRAM: &str = "phantomjs";

/// The PhantomJS release the stand-in answers `-v` with: the last, which yt-dlp asks for at least.
pub const VERSION: &str = "2.1.1";

/// Names the Deno the stand-in runs scripts in.
pub const DENO_VARIABLE: &str = "WIRECATCH_PHANTOMJS_DENO";

/// Names the run's own folder, where yt-dlp keeps its temporary files: a script may read and write only the files directly in it.
pub const FOLDER_VARIABLE: &str = "WIRECATCH_PHANTOMJS_FOLDER";

/// The stand-in installed with the app: in a `phantomjs` folder of its own beside the app on Windows, under `/usr/lib/<slug>` beside `/usr/bin` on Linux. `None` when it is not there, as in a development build.
#[must_use]
pub fn installed() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let file = format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX);
    [
        dir.join(PROGRAM).join(&file),
        dir.join("..")
            .join("lib")
            .join(PRODUCT_SLUG)
            .join(PROGRAM)
            .join(&file),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// What a run of yt-dlp gets to find the stand-in at `program`: its folder first on `PATH`, the Deno it runs scripts in, and `folder`, the one place they may touch, which is also the run's temporary folder.
pub(crate) fn environment(
    program: &Path,
    deno: &Path,
    folder: &Path,
) -> Vec<(OsString, Option<OsString>)> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = program.parent().map_or_else(
        || current.clone(),
        |dir| {
            std::env::join_paths(
                std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&current)),
            )
            .unwrap_or_else(|_| current.clone())
        },
    );
    vec![
        ("PATH".into(), Some(path)),
        (DENO_VARIABLE.into(), Some(deno.into())),
        (FOLDER_VARIABLE.into(), Some(folder.into())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_comes_first_on_the_path() {
        let program = Path::new("app").join(PROGRAM).join(PROGRAM);
        let environment = environment(&program, Path::new("deno"), Path::new("run"));
        let path = environment
            .iter()
            .find(|(name, _)| name == "PATH")
            .and_then(|(_, value)| value.clone())
            .unwrap();
        assert_eq!(
            std::env::split_paths(&path).next(),
            Some(Path::new("app").join(PROGRAM))
        );
        assert!(environment.contains(&(FOLDER_VARIABLE.into(), Some("run".into()))));
        assert!(environment.contains(&(DENO_VARIABLE.into(), Some("deno".into()))));
    }
}
