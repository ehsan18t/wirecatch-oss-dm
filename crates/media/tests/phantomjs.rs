//! The PhantomJS stand-in (D281) driven as yt-dlp's `PhantomJSwrapper` drives PhantomJS, in the real Deno the media add-on installs (`WIRECATCH_TEST_DENO`). `testdata/phantomjs` holds the wrapper's own scripts, from `yt_dlp/extractor/openload.py`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wirecatch_media::phantomjs::{DENO_VARIABLE, FOLDER_VARIABLE, PROGRAM, VERSION};
use wirecatch_test_support::{TestResult, private_dir, real_deno};

const STAND_IN: &str = env!("CARGO_BIN_EXE_phantomjs");
const BASE_JS: &str = include_str!("../testdata/phantomjs/base.js.txt");
const TEMPLATE: &str = include_str!("../testdata/phantomjs/template.js.txt");
const CHECK_PAGE: &str = include_str!("../testdata/browser-check.html");

/// The stand-in run on `script` as the wrapper runs PhantomJS, with `folder` the run's own.
fn run(deno: &Path, folder: &Path, script: &Path) -> TestResult<Output> {
    Ok(Command::new(STAND_IN)
        .arg("--ssl-protocol=any")
        .arg(script)
        .env(DENO_VARIABLE, deno)
        .env(FOLDER_VARIABLE, folder)
        .output()?)
}

/// What the wrapper's `execute` runs for `jscode`.
fn executed(jscode: &str) -> String {
    let jscode = if jscode.contains("phantom.exit();") {
        jscode.to_owned()
    } else {
        format!("{jscode};\nphantom.exit();")
    };
    format!("{BASE_JS}{jscode}")
}

/// A path as the wrapper writes one into its template.
fn quoted(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// The wrapper's files for `get`: the page, the cookies it hands over, and the script.
struct Get {
    html: PathBuf,
    cookies: PathBuf,
    script: PathBuf,
}

/// Lays out what the wrapper's `get` writes before it runs PhantomJS, as Python's `format_map` fills the template.
fn get(
    folder: &Path,
    html: &str,
    cookies: &str,
    url: &str,
    ua: &str,
    jscode: &str,
) -> TestResult<Get> {
    let files = Get {
        html: folder.join("html"),
        cookies: folder.join("cookies"),
        script: folder.join("script"),
    };
    std::fs::write(&files.html, html)?;
    std::fs::write(&files.cookies, cookies)?;
    let filled = TEMPLATE
        .replace("{{", "\u{1}")
        .replace("}}", "\u{2}")
        .replace("{cookies}", &quoted(&files.cookies))
        .replace("{html}", &quoted(&files.html))
        .replace("{timeout}", "10000")
        .replace("{ua}", ua)
        .replace("{url}", url)
        .replace("{jscode}", jscode)
        .replace('\u{1}', "{")
        .replace('\u{2}', "}");
    std::fs::write(&files.script, executed(&filled))?;
    Ok(files)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// The app and the installers find the program by this name, and yt-dlp looks for no other.
#[test]
fn the_program_has_the_name_yt_dlp_looks_for() {
    assert_eq!(
        Path::new(STAND_IN).file_stem().and_then(|s| s.to_str()),
        Some(PROGRAM)
    );
}

#[test]
fn it_answers_as_the_last_phantomjs() {
    let output = Command::new(STAND_IN).arg("-v").output().unwrap();
    assert!(output.status.success());
    assert_eq!(stdout(&output), VERSION);
}

/// Douyu signs its request with a script whose printed line yt-dlp reads.
#[test]
fn a_script_runs_and_its_output_is_read() {
    let Some(deno) = real_deno() else { return };
    let folder = private_dir("phantomjs").unwrap();
    let script = folder.path().join("script");
    std::fs::write(
        &script,
        executed(
            r#"function ub98484234(a, b, c) { return "v=" + a + "&did=" + b + "&tt=" + c; }; console.log(ub98484234("1", "2", "3"))"#,
        ),
    )
    .unwrap();
    let output = run(&deno, folder.path(), &script).unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "v=1&did=2&tt=3");
}

/// A site's check page sets its cookie as it loads, and the cookie goes back to yt-dlp with the ones it handed over.
#[test]
fn a_check_page_gives_back_its_cookie() {
    let Some(deno) = real_deno() else { return };
    let folder = private_dir("phantomjs").unwrap();
    let files = get(
        folder.path(),
        CHECK_PAGE,
        r#"[{"name": "session", "value": "abc", "domain": ".example.com", "path": "/"}]"#,
        "https://www.example.com/view_video.php?viewkey=1",
        "Mozilla/5.0 (test)",
        "saveAndExit();",
    )
    .unwrap();
    let output = run(&deno, folder.path(), &files.script).unwrap();
    assert!(output.status.success(), "{output:?}");
    let cookies: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&files.cookies).unwrap()).unwrap();
    let named = |name: &str| cookies.iter().find(|c| c["name"] == name).cloned();
    let key = named("KEY").unwrap();
    assert_eq!(key["value"], "3011*763245449:3657345595:1981505470:1");
    assert_eq!(key["domain"], "www.example.com");
    assert_eq!(key["httponly"], false);
    assert_eq!(named("session").unwrap()["httponly"], false);
    assert!(
        std::fs::read_to_string(&files.html)
            .unwrap()
            .contains("leastFactor")
    );
}

/// iQiyi works its signature out inside a blank page and prints what the page gave back.
#[test]
fn a_page_evaluates_and_gives_back_a_copy() {
    let Some(deno) = real_deno() else { return };
    let folder = private_dir("phantomjs").unwrap();
    let files = get(
        folder.path(),
        "<!DOCTYPE html>",
        "[]",
        "https://www.example.com/play",
        "Mozilla/5.0 (test)",
        r"console.log(page.evaluate(function() {
            var doubled = [1, 2].map(function(n) { return n * 2; });
            return JSON.stringify({ ua: navigator.userAgent, host: location.hostname, doubled: doubled });
        }));
        saveAndExit();",
    )
    .unwrap();
    let output = run(&deno, folder.path(), &files.script).unwrap();
    assert!(output.status.success(), "{output:?}");
    let said: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(
        said,
        serde_json::json!({ "ua": "Mozilla/5.0 (test)", "host": "www.example.com", "doubled": [2, 4] })
    );
}

/// A script reads and writes only in the run's folder, and one from elsewhere is not run.
#[test]
fn a_script_stays_in_its_folder() {
    let Some(deno) = real_deno() else { return };
    let folder = private_dir("phantomjs").unwrap();
    let elsewhere = private_dir("elsewhere").unwrap();
    let secret = elsewhere.path().join("secret");
    std::fs::write(&secret, "not for the page").unwrap();
    let script = folder.path().join("script");
    std::fs::write(
        &script,
        executed(&format!(
            "console.log(require('fs').read(\"{}\"))",
            quoted(&secret)
        )),
    )
    .unwrap();
    let output = run(&deno, folder.path(), &script).unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(!stdout(&output).contains("not for the page"));

    let outside = elsewhere.path().join("script");
    std::fs::write(&outside, executed("console.log('ran')")).unwrap();
    let output = run(&deno, folder.path(), &outside).unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(stdout(&output), "");
}

#[test]
fn without_a_deno_nothing_runs() {
    let folder = private_dir("phantomjs").unwrap();
    let script = folder.path().join("script");
    std::fs::write(&script, executed("console.log('ran')")).unwrap();
    let output = Command::new(STAND_IN)
        .arg(&script)
        .env_remove(DENO_VARIABLE)
        .env(FOLDER_VARIABLE, folder.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Deno"));
}
