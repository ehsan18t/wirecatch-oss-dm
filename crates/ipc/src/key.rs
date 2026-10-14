//! The per-user endpoint key: a random part of the endpoint's name, kept in a file only
//! the user can read, so another account cannot work out the name and take it before
//! Wirecatch starts (IPB-10, D41.4 to D41.6).
//!
//! Only the listener writes the key, and only while it holds the key folder's lock, which
//! it keeps for as long as it listens: the lock is also what stops a second copy of the app
//! (a copy with a new key would otherwise bind a pipe of its own). Clients only read the
//! key; a missing key means the app has not listened yet for this account.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::IpcError;

/// The key file's name inside the key folder.
pub(crate) const KEY_FILE: &str = "endpoint-key";

/// The lock file's name inside the key folder.
const LOCK_FILE: &str = "endpoint-key.lock";

/// Where the listener writes a new key before it renames it into place.
const STAGED_FILE: &str = "endpoint-key.tmp";

/// Random bytes in a key: 128 bits, more than anyone can guess.
const KEY_BYTES: usize = 16;

/// Characters in a key's text: two lower-case hexadecimal digits per byte.
const KEY_CHARS: usize = KEY_BYTES * 2;

/// The random part of the endpoint's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EndpointKey(String);

impl EndpointKey {
    /// A new key from the operating system's random source.
    fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; KEY_BYTES];
        getrandom::fill(&mut bytes).map_err(io::Error::from)?;
        let mut text = String::with_capacity(KEY_CHARS);
        for byte in bytes {
            write!(text, "{byte:02x}").map_err(io::Error::other)?;
        }
        Ok(Self(text))
    }

    /// Reads a key's text: exactly [`KEY_CHARS`] lower-case hexadecimal digits.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let well_formed = text.len() == KEY_CHARS
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        well_formed.then(|| Self(text.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the key file in a folder holds.
#[derive(Debug)]
enum Stored {
    Missing,
    Damaged,
    Key(EndpointKey),
}

/// The key in `folder`, for a client. Never writes anything.
///
/// # Errors
///
/// [`IpcError::NoEndpointKey`] when there is none yet, [`IpcError::BadEndpointKey`] when the
/// file is damaged or not private.
pub(crate) fn read(folder: &Path) -> Result<EndpointKey, IpcError> {
    let path = folder.join(KEY_FILE);
    match stored(&path)? {
        Stored::Key(key) => Ok(key),
        Stored::Missing => Err(IpcError::NoEndpointKey { path }),
        Stored::Damaged => Err(bad_key(
            &path,
            "it does not hold a key Wirecatch wrote".into(),
        )),
    }
}

/// The key folder's lock, held by the one listener of the folder's channel for as long as
/// it listens. The operating system releases it when the process ends, however it ends.
#[derive(Debug)]
pub(crate) struct KeyLock {
    folder: PathBuf,
    _file: File,
}

impl KeyLock {
    /// Takes the lock of `folder`, creating the folder when it is missing.
    ///
    /// # Errors
    ///
    /// [`IpcError::AlreadyInUse`] when another listener holds it, [`IpcError::Io`] when the
    /// folder or the lock file cannot be made.
    pub(crate) fn take(folder: &Path) -> Result<Self, IpcError> {
        create_folder(folder)?;
        let path = folder.join(LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| IpcError::io(format!("Could not open {}", path.display()), e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                folder: folder.to_path_buf(),
                _file: file,
            }),
            Err(TryLockError::WouldBlock) => Err(IpcError::AlreadyInUse {
                endpoint: path.display().to_string(),
            }),
            Err(TryLockError::Error(e)) => Err(IpcError::io(
                format!("Could not lock {}", path.display()),
                e,
            )),
        }
    }

    /// The key to listen with: the one in the folder, or a new one when there is none or
    /// the file is damaged. Replacing a damaged key is safe here, since no other listener
    /// can be using it while this one holds the lock. A key file that is not private is
    /// refused, not replaced: someone else put it there.
    pub(crate) fn current(&self) -> Result<EndpointKey, IpcError> {
        match stored(&self.key_path())? {
            Stored::Key(key) => Ok(key),
            Stored::Missing | Stored::Damaged => self.renew(),
        }
    }

    /// Writes a new key in place of the current one, for when the name it gives is taken.
    pub(crate) fn renew(&self) -> Result<EndpointKey, IpcError> {
        let key = EndpointKey::generate()
            .map_err(|e| IpcError::io("Could not make a key for the connection point", e))?;
        let path = self.key_path();
        let staged = self.folder.join(STAGED_FILE);
        // Written aside and renamed over the old key, so a client reading it at the same
        // moment sees the old key or the new one, never a part. Only the holder of the lock
        // writes here, so a staged copy is always one an earlier holder left when it failed.
        let written = remove_if_present(&staged)
            .and_then(|()| write_private(&staged, key.as_str()))
            .and_then(|()| fs::rename(&staged, &path));
        written.map_err(|e| {
            IpcError::io(
                format!(
                    "Could not save the connection point's key in {}",
                    path.display()
                ),
                e,
            )
        })?;
        Ok(key)
    }

    fn key_path(&self) -> PathBuf {
        self.folder.join(KEY_FILE)
    }
}

/// Removes the file at `path`, if there is one.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn create_folder(folder: &Path) -> Result<(), IpcError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(folder).map_err(|e| {
        IpcError::io(
            format!("Could not create the folder {}", folder.display()),
            e,
        )
    })
}

/// What the file at `path` holds. A file that is not private is an error, not a damaged
/// key: it is refused, never replaced.
fn stored(path: &Path) -> Result<Stored, IpcError> {
    let file = match open_for_reading(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Stored::Missing),
        Err(e) => {
            return Err(IpcError::io(
                format!("Could not open {}", path.display()),
                e,
            ));
        }
    };
    check_private(&file).map_err(|reason| bad_key(path, reason))?;
    // One byte more than a key, so a longer file is seen as one without reading it all.
    let mut text = String::new();
    let limit = u64::try_from(KEY_CHARS + 1).unwrap_or(u64::MAX);
    match file.take(limit).read_to_string(&mut text) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::InvalidData => return Ok(Stored::Damaged),
        Err(e) => {
            return Err(IpcError::io(
                format!("Could not read {}", path.display()),
                e,
            ));
        }
    }
    Ok(EndpointKey::parse(&text).map_or(Stored::Damaged, Stored::Key))
}

fn bad_key(path: &Path, reason: String) -> IpcError {
    IpcError::BadEndpointKey {
        path: path.to_path_buf(),
        reason,
    }
}

/// Opens the key without following a link in its place: a link could point at a file
/// another account controls.
#[cfg(unix)]
fn open_for_reading(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

/// Opens the key without following a link in its place (a symbolic link or a junction):
/// with `FILE_FLAG_OPEN_REPARSE_POINT` the link itself opens, which [`check_private`] then
/// refuses.
#[cfg(windows)]
fn open_for_reading(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Refuses a key file that is a link, not a plain file, not the user's own, or (Linux)
/// readable by other accounts: the key would no longer be a secret from them.
fn check_private(file: &File) -> Result<(), String> {
    let meta = file
        .metadata()
        .map_err(|e| format!("its details could not be read ({e})"))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err("it is a link to another file".into());
        }
    }
    if !meta.is_file() {
        return Err("it is not a plain file".into());
    }
    #[cfg(windows)]
    {
        let owner = crate::windows::file_owner(file)
            .map_err(|e| format!("its owner could not be read ({e})"))?;
        let user = crate::windows::Sid::current_user().map_err(|e| e.to_string())?;
        if !crate::windows::may_own_users_file(&owner, &user) {
            return Err(format!(
                "it belongs to another account ({owner}) instead of you ({user})"
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let uid = crate::unix::current_uid();
        if meta.uid() != uid {
            return Err(format!(
                "it belongs to user {} instead of you (user {uid})",
                meta.uid()
            ));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(format!(
                "other users can read it (permissions {:03o}, expected 600)",
                meta.mode() & 0o777
            ));
        }
    }
    Ok(())
}

/// Writes `text` to a new file at `path` that only the user can read, and that the user owns even when the program is elevated.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    #[cfg(windows)]
    let mut file = crate::windows::create_private_file(path)?;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?
    };
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::private_dir;

    #[test]
    fn a_key_is_128_random_bits_in_hexadecimal() {
        let first = EndpointKey::generate().unwrap();
        let second = EndpointKey::generate().unwrap();
        assert_ne!(first, second);
        assert_eq!(first.as_str().len(), KEY_CHARS);
        assert_eq!(EndpointKey::parse(first.as_str()), Some(first));
    }

    #[test]
    fn only_a_key_wirecatch_writes_is_read() {
        let good = "0123456789abcdef0123456789abcdef";
        assert!(EndpointKey::parse(good).is_some());
        for bad in [
            "",
            "0123456789abcdef",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcdeg",
            "0123456789abcdef0123456789abcdef0",
            "0123456789abcdef0123456789abcdef\n",
            "../../../../../../../../../../..",
        ] {
            assert_eq!(EndpointKey::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_listener_writes_the_key_once_and_keeps_it() {
        let dir = private_dir("ipc-key").unwrap();
        let folder = dir.path().join("Wirecatch");
        let lock = KeyLock::take(&folder).unwrap();
        let key = lock.current().unwrap();
        assert_eq!(lock.current().unwrap(), key, "kept across reads");
        assert_eq!(read(&folder).unwrap(), key, "what clients read");
        drop(lock);
        let again = KeyLock::take(&folder).unwrap();
        assert_eq!(again.current().unwrap(), key, "kept across restarts");
        // Nothing is left of the step on the way.
        let mut names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, [KEY_FILE, LOCK_FILE]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(folder.join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_renewed_key_replaces_the_old_one() {
        let dir = private_dir("ipc-key-renew").unwrap();
        let lock = KeyLock::take(dir.path()).unwrap();
        let old = lock.current().unwrap();
        let new = lock.renew().unwrap();
        assert_ne!(old, new);
        assert_eq!(read(dir.path()).unwrap(), new);
    }

    // The lock is the second-copy guard: a second listener with a new key would bind a
    // name of its own and never hear that the first one runs (W4I review M1).
    #[test]
    fn a_second_listener_cannot_take_the_lock() {
        let dir = private_dir("ipc-key-lock").unwrap();
        let first = KeyLock::take(dir.path()).unwrap();
        assert!(matches!(
            KeyLock::take(dir.path()),
            Err(IpcError::AlreadyInUse { .. })
        ));
        drop(first);
        KeyLock::take(dir.path()).expect("free once the first listener stops");
    }

    // Under the lock no other listener can be using a damaged key, so the listener writes a
    // new one; a client refuses it and writes nothing.
    #[test]
    fn a_damaged_key_is_refused_by_clients_and_replaced_by_the_listener() {
        let dir = private_dir("ipc-key-bad").unwrap();
        let path = dir.path().join(KEY_FILE);
        for damaged in ["", "not a key", &"a".repeat(10_000)] {
            remove_if_present(&path).unwrap();
            write_private(&path, damaged).unwrap();
            let err = read(dir.path()).unwrap_err();
            assert!(matches!(err, IpcError::BadEndpointKey { .. }), "{err:?}");
            assert!(err.to_string().contains("delete"), "{err}");
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                damaged,
                "a client wrote"
            );

            let lock = KeyLock::take(dir.path()).unwrap();
            let key = lock.current().unwrap();
            assert_eq!(fs::read_to_string(&path).unwrap(), key.as_str());
        }
    }

    #[test]
    fn a_folder_in_place_of_the_key_is_refused() {
        let dir = private_dir("ipc-key-folder").unwrap();
        fs::create_dir(dir.path().join(KEY_FILE)).unwrap();
        assert!(read(dir.path()).is_err());
        let lock = KeyLock::take(dir.path()).unwrap();
        assert!(lock.current().is_err(), "the listener does not replace it");
    }

    // On Windows the key used to be read through a link, to wherever it pointed (W4I
    // review L3). Making a symbolic link needs developer mode or an administrator, so the
    // test says so and stops when it cannot make one.
    #[cfg(windows)]
    #[test]
    fn a_link_in_place_of_the_key_is_refused_on_windows() {
        let dir = private_dir("ipc-key-link").unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::write(&elsewhere, "0123456789abcdef0123456789abcdef").unwrap();
        if let Err(e) = std::os::windows::fs::symlink_file(&elsewhere, dir.path().join(KEY_FILE)) {
            eprintln!("cannot make a symbolic link here ({e}): nothing to check");
            return;
        }
        let err = read(dir.path()).unwrap_err();
        assert!(err.to_string().contains("link"), "{err}");
    }

    // A key an elevated program wrote without naming its owner (an earlier Wirecatch run elevated, or with UAC off) belongs to Administrators, and was refused by every later run, the listener included, which could not replace it either (W4I review R2-H1).
    #[cfg(windows)]
    #[test]
    fn a_key_administrators_own_is_read_and_kept() {
        let dir = private_dir("ipc-key-admin").unwrap();
        let path = dir.path().join(KEY_FILE);
        let text = "0123456789abcdef0123456789abcdef";
        let Some(written) = crate::windows::as_an_elevated_program(|| fs::write(&path, text))
        else {
            eprintln!(
                "this program is not elevated, so it cannot make a file Administrators own: nothing to check"
            );
            return;
        };
        written.unwrap();
        let owner = crate::windows::file_owner(&File::open(&path).unwrap()).unwrap();
        assert_eq!(owner, crate::windows::Sid::administrators(), "the setup");

        assert_eq!(
            read(dir.path()).unwrap().as_str(),
            text,
            "what clients read"
        );
        let lock = KeyLock::take(dir.path()).unwrap();
        assert_eq!(
            lock.current().unwrap().as_str(),
            text,
            "the listener keeps it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_key_others_can_read_or_a_link_in_its_place_is_refused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = private_dir("ipc-key-unix").unwrap();
        let path = dir.path().join(KEY_FILE);
        fs::write(&path, "0123456789abcdef0123456789abcdef").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let err = read(dir.path()).unwrap_err();
        assert!(err.to_string().contains("other users can read it"), "{err}");
        let lock = KeyLock::take(dir.path()).unwrap();
        assert!(lock.current().is_err(), "the listener does not replace it");
        drop(lock);

        fs::remove_file(&path).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::write(&elsewhere, "0123456789abcdef0123456789abcdef").unwrap();
        fs::set_permissions(&elsewhere, fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        assert!(read(dir.path()).is_err(), "a link is not followed");
    }
}
