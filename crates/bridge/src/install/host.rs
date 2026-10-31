//! The file system and registry a registration is written to.

use std::fs;
use std::io;
use std::path::Path;

use super::plan::Hive;

/// File system and registry access, behind a trait so tests can run against a
/// temporary folder and an in-memory registry.
pub(crate) trait Host {
    /// The file's contents, or `None` if it does not exist.
    fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>>;
    /// Whether a file exists at `path`.
    fn is_file(&self, path: &Path) -> bool;
    /// Writes the file, creating missing folders, replacing it atomically.
    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()>;
    /// Removes the file.
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    /// Removes the folder if it exists and is empty. Returns whether it did.
    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool>;
    /// The default value of a key, or `None` if the key or value is missing.
    fn registry_value(&self, hive: Hive, key: &str) -> io::Result<Option<String>>;
    /// Creates the key (and missing parents) and sets its default value.
    fn set_registry_value(&mut self, hive: Hive, key: &str, value: &str) -> io::Result<()>;
    /// Deletes the key, which must have no subkeys. Missing is fine.
    fn delete_registry_key(&mut self, hive: Hive, key: &str) -> io::Result<()>;
    /// Deletes the key if it exists with no subkeys and no values. Returns
    /// whether it did.
    fn delete_registry_key_if_empty(&mut self, hive: Hive, key: &str) -> io::Result<bool>;
}

/// The real file system and (on Windows) registry.
pub(crate) struct RealHost;

pub(crate) mod files {
    //! File operations shared by the real host and the test host.

    use super::{Path, fs, io};

    pub(crate) fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        fs::create_dir_all(dir)?;
        // Named by process: two app starts can register at the same time.
        let mut temp_name = path.file_name().unwrap_or_default().to_os_string();
        temp_name.push(format!(".{}.tmp", std::process::id()));
        let temp = dir.join(temp_name);
        let result = fs::write(&temp, contents).and_then(|()| fs::rename(&temp, path));
        if result.is_err()
            && let Err(e) = fs::remove_file(&temp)
        {
            tracing::warn!(
                "could not remove the temporary file {}: {e}",
                temp.display()
            );
        }
        result
    }

    pub(crate) fn remove_dir_if_empty(path: &Path) -> io::Result<bool> {
        // remove_dir only succeeds on an empty folder, so the check and the
        // removal are one step.
        match fs::remove_dir(path) {
            Ok(()) => Ok(true),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
}

impl Host for RealHost {
    fn read_file(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        files::read(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn write_file(&mut self, path: &Path, contents: &[u8]) -> io::Result<()> {
        files::write(path, contents)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn remove_dir_if_empty(&mut self, path: &Path) -> io::Result<bool> {
        files::remove_dir_if_empty(path)
    }

    fn registry_value(&self, hive: Hive, key: &str) -> io::Result<Option<String>> {
        registry::value(hive, key)
    }

    fn set_registry_value(&mut self, hive: Hive, key: &str, value: &str) -> io::Result<()> {
        registry::set_value(hive, key, value)
    }

    fn delete_registry_key(&mut self, hive: Hive, key: &str) -> io::Result<()> {
        registry::delete_key(hive, key)
    }

    fn delete_registry_key_if_empty(&mut self, hive: Hive, key: &str) -> io::Result<bool> {
        registry::delete_key_if_empty(hive, key)
    }
}

#[cfg(not(windows))]
pub(super) mod registry {
    //! There is no registry outside Windows; the Linux plan never uses it.

    use std::io;

    use super::Hive;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "there is no registry on this system",
        )
    }

    pub(crate) fn value(_hive: Hive, _key: &str) -> io::Result<Option<String>> {
        Err(unsupported())
    }
    pub(crate) fn set_value(_hive: Hive, _key: &str, _value: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key(_hive: Hive, _key: &str) -> io::Result<()> {
        Err(unsupported())
    }
    pub(crate) fn delete_key_if_empty(_hive: Hive, _key: &str) -> io::Result<bool> {
        Err(unsupported())
    }
}

#[cfg(windows)]
pub(super) mod registry {
    //! The registry (`HKEY_CURRENT_USER` or `HKEY_LOCAL_MACHINE`), through `windows-registry`.

    use std::io;

    use windows_registry::{CURRENT_USER, Key, LOCAL_MACHINE};
    use windows_result::HRESULT;
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;

    use super::Hive;

    fn root(hive: Hive) -> &'static Key {
        match hive {
            Hive::CurrentUser => CURRENT_USER,
            Hive::LocalMachine => LOCAL_MACHINE,
        }
    }

    /// `None` for a key or value that does not exist.
    fn found<T>(result: windows_registry::Result<T>) -> io::Result<Option<T>> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(e) if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub(crate) fn value(hive: Hive, key: &str) -> io::Result<Option<String>> {
        match found(root(hive).open(key))? {
            Some(key) => found(key.get_string("")),
            None => Ok(None),
        }
    }

    pub(crate) fn set_value(hive: Hive, key: &str, value: &str) -> io::Result<()> {
        root(hive)
            .create(key)
            .and_then(|key| key.set_string("", value))
            .map_err(io::Error::from)
    }

    /// Refuses a key with subkeys: `remove_tree`, the crate's only way to delete a key, would
    /// take them too.
    pub(crate) fn delete_key(hive: Hive, key: &str) -> io::Result<()> {
        let Some(opened) = found(root(hive).open(key))? else {
            return Ok(());
        };
        if opened.keys()?.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                format!("{key} has subkeys"),
            ));
        }
        drop(opened);
        found(root(hive).remove_tree(key)).map(drop)
    }

    pub(crate) fn delete_key_if_empty(hive: Hive, key: &str) -> io::Result<bool> {
        let Some(opened) = found(root(hive).open(key))? else {
            return Ok(false);
        };
        if opened.keys()?.next().is_some() || opened.values()?.next().is_some() {
            return Ok(false);
        }
        drop(opened);
        delete_key(hive, key)?;
        Ok(true)
    }
}
