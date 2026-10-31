//! Windows transport: a per-user named pipe with an explicit owner-only security
//! descriptor, and a connector that checks the pipe owner before using it.

use std::ffi::{OsStr, c_void};
use std::fmt;
use std::fs::File;
use std::io;
use std::iter;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, PipeMode, ServerOptions};
use tokio::time::{Instant, sleep};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_CANNOT_IMPERSONATE, ERROR_CANT_OPEN_ANONYMOUS, ERROR_FILE_NOT_FOUND,
    ERROR_PIPE_BUSY, ERROR_SUCCESS, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree, TRUE,
    WIN32_ERROR,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    CopySid, GetLengthSid, GetTokenInformation, IsTokenRestricted, IsValidSid,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf, SECURITY_ATTRIBUTES,
    SID_AND_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_ELEVATION_TYPE, TOKEN_INFORMATION_CLASS,
    TOKEN_QUERY, TokenElevation, TokenElevationType, TokenElevationTypeDefault as SINGLE_TOKEN,
    TokenElevationTypeFull as FULL_TOKEN, TokenElevationTypeLimited as LIMITED_TOKEN,
    TokenIntegrityLevel, TokenIsAppContainer, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, SECURITY_IDENTIFICATION,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};
use wirecatch_product::Channel;

use crate::key::EndpointKey;
use crate::{Connection, Elevation, Endpoint, Inner, IpcError, paths};

/// How long `connect` keeps retrying while every pipe instance is busy. The
/// listener always keeps one instance waiting, so this only covers the short gap
/// while it creates the next one.
const BUSY_WAIT: Duration = Duration::from_secs(2);

/// How long `connect` waits before trying a busy pipe again.
const BUSY_RETRY: Duration = Duration::from_millis(10);

/// The longest string form a SID has, in UTF-16 code units: `S-1-`, an identifier
/// authority of at most 15 characters, and up to 15 sub-authorities of up to 10
/// digits each, every one after a dash.
const MAX_SID_STRING_UNITS: usize = 4 + 15 + 15 * 11;

/// The size of `T` as the `u32` that Win32 structures and calls take. A type of 4 GiB
/// or more does not compile, so the conversion below never falls back.
fn win32_size_of<T>() -> u32 {
    const { assert!(mem::size_of::<T>() <= u32::MAX as usize) };
    u32::try_from(mem::size_of::<T>()).unwrap_or(u32::MAX)
}

/// Whether `error` is the Win32 error `code`.
fn is_os_error(error: &io::Error, code: WIN32_ERROR) -> bool {
    error.raw_os_error() == Some(code.cast_signed())
}

/// The Win32 error `code` as an `io::Error`.
fn os_error(code: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(code.cast_signed())
}

/// Whether the endpoint's name needs a key: always, since pipe names are machine-wide and
/// another account that knows the user's SID (which is no secret) could create the pipe
/// first. Windows has no runtime folder.
pub(crate) fn needs_key(_runtime: Option<&Path>) -> bool {
    true
}

/// `channel`'s pipe for the current user, named with the endpoint key that `key` gives.
/// Windows has no runtime folder, so the name always carries the key.
pub(crate) fn endpoint(
    channel: Channel,
    _runtime: Option<&Path>,
    key: impl FnOnce() -> Result<EndpointKey, IpcError>,
) -> Result<Endpoint, IpcError> {
    let sid = Sid::current_user()?
        .to_string_form()
        .map_err(|e| IpcError::io("Could not read your user account", e))?;
    let key = key()?;
    Ok(Endpoint::keyed(
        paths::pipe_name(channel, &sid, key.as_str()),
        &key,
    ))
}

/// The user's local application data folder, `%LOCALAPPDATA%`.
pub(crate) fn local_data_base() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// A Windows security identifier: who an account is. It prints in its string form, for
/// example `S-1-5-21-...-1001`.
///
/// Stored as 32-bit words because a SID is always 8 bytes plus 4 per
/// sub-authority, which keeps the buffer aligned for the APIs that read it.
#[derive(Clone, PartialEq, Eq)]
pub struct Sid {
    words: Vec<u32>,
}

impl Sid {
    /// The user the current process runs as.
    pub(crate) fn current_user() -> Result<Self, IpcError> {
        process_token()
            .and_then(|token| token_sid(&token, TokenUser))
            .map_err(|e| IpcError::io("Could not read your user account", e))
    }

    /// The integrity level the current process runs at, as its mandatory label SID: `S-1-16-8192` for a normal (medium) process, `S-1-16-12288` for an elevated (high) one.
    pub(crate) fn current_integrity() -> Result<Self, IpcError> {
        process_token()
            .and_then(|token| token_sid(&token, TokenIntegrityLevel))
            .map_err(|e| IpcError::io("Could not read how this program was started", e))
    }

    /// The level a mandatory label SID names (`S-1-16-<level>`: 4096 low, 8192
    /// medium, 12288 high), or `None` for any other SID. Read from the SID's layout:
    /// a revision byte, a sub-authority count byte, a six-byte big-endian identifier
    /// authority, then the sub-authorities.
    fn integrity_level(&self) -> Option<u32> {
        const MANDATORY_LABEL_AUTHORITY: [u8; 6] = [0, 0, 0, 0, 0, 16];
        let [head, authority_tail, level] = self.words[..] else {
            return None;
        };
        let [_revision, count, a0, a1] = head.to_le_bytes();
        let [a2, a3, a4, a5] = authority_tail.to_le_bytes();
        (count == 1 && [a0, a1, a2, a3, a4, a5] == MANDATORY_LABEL_AUTHORITY).then_some(level)
    }

    /// The SID `S-1-5-<sub_authorities>` of the NT authority, laid out as [`Self::integrity_level`] reads one.
    fn nt_authority<const N: usize>(sub_authorities: [u32; N]) -> Self {
        const NT_AUTHORITY: u8 = 5;
        const {
            assert!(N > 0);
            assert!(N <= 15);
        };
        let count = u8::try_from(N).unwrap_or(u8::MAX);
        let mut words = vec![
            u32::from_le_bytes([1, count, 0, 0]),
            u32::from_le_bytes([0, 0, 0, NT_AUTHORITY]),
        ];
        words.extend(sub_authorities);
        Self { words }
    }

    /// `BUILTIN\Administrators`, `S-1-5-32-544`.
    pub(crate) fn administrators() -> Self {
        Self::nt_authority([32, 544])
    }

    /// The operating system's own account (`LocalSystem`), `S-1-5-18`.
    pub(crate) fn local_system() -> Self {
        Self::nt_authority([18])
    }

    /// Copies the SID at `psid`.
    ///
    /// # Safety
    ///
    /// `psid` must be null or point at memory that holds a SID for the duration of
    /// the call.
    unsafe fn copy_from(psid: PSID) -> io::Result<Self> {
        // SAFETY: the caller guarantees `psid` is null or readable; IsValidSid
        // checks the structure before anything relies on its length.
        if psid.is_null() || unsafe { IsValidSid(psid) } == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an invalid security identifier",
            ));
        }
        // SAFETY: `psid` is a valid SID (checked above).
        let len = unsafe { GetLengthSid(psid) };
        let mut words = vec![0u32; (len as usize).div_ceil(4)];
        // SAFETY: `words` holds at least `len` bytes; CopySid writes at most `len`.
        if unsafe { CopySid(len, words.as_mut_ptr().cast(), psid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { words })
    }

    /// Parses the string form, for example `S-1-5-18`.
    ///
    /// # Errors
    ///
    /// Windows does not read `text` as a SID.
    pub fn parse(text: &str) -> io::Result<Self> {
        use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;

        let wide = wide(OsStr::new(text));
        let mut psid: PSID = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; on success `psid` receives a
        // LocalAlloc block that the guard frees.
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &raw mut psid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _guard = LocalBox(psid);
        // SAFETY: `psid` points at the SID Windows just built.
        unsafe { Self::copy_from(psid) }
    }

    /// Windows only reads through this pointer.
    fn as_psid(&self) -> PSID {
        self.words.as_ptr() as PSID
    }

    /// The string form, for example `S-1-5-21-...-1001`.
    pub(crate) fn to_string_form(&self) -> io::Result<String> {
        let mut out: *mut u16 = ptr::null_mut();
        // SAFETY: `self` holds a valid SID; on success `out` receives a
        // NUL-terminated LocalAlloc string that the guard frees.
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &raw mut out) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _guard = LocalBox(out.cast());
        // SAFETY: `out` is NUL-terminated, so the scan stops inside the string; it
        // also stops at the longest string form a SID has.
        let len = (0..=MAX_SID_STRING_UNITS)
            .take_while(|&i| unsafe { *out.add(i) } != 0)
            .count();
        if len > MAX_SID_STRING_UNITS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Windows returned an account id longer than any account id can be",
            ));
        }
        // SAFETY: the first `len` code units of `out` are initialized.
        let units = unsafe { std::slice::from_raw_parts(out, len) };
        Ok(String::from_utf16_lossy(units))
    }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to_string_form() {
            Ok(text) => f.write_str(&text),
            Err(_) => f.write_str("<unreadable account id>"),
        }
    }
}

impl fmt::Debug for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sid({self})")
    }
}

/// The current process's access token, for reading only.
fn process_token() -> io::Result<OwnedHandle> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing and is
    // valid for the whole life of the process.
    let current = unsafe { BorrowedHandle::borrow_raw(GetCurrentProcess() as RawHandle) };
    token_of(current)
}

/// The access token of `process`, for reading only. The handle needs
/// `PROCESS_QUERY_LIMITED_INFORMATION`.
fn token_of(process: BorrowedHandle<'_>) -> io::Result<OwnedHandle> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: `process` is open for the duration of the call (it is borrowed); on
    // success `raw` is a new token handle that we own.
    if unsafe { OpenProcessToken(process.as_raw_handle() as HANDLE, TOKEN_QUERY, &raw mut raw) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a valid handle we own and nothing else closes it.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) })
}

/// The account a Windows process runs as: its user and its integrity level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessAccount {
    /// The user.
    pub user: Sid,
    /// The integrity level: 4096 low, 8192 medium, 12288 high (elevated), 16384
    /// system.
    pub integrity: u32,
}

impl ProcessAccount {
    /// The account the current process runs as.
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when Windows cannot report the process's token.
    pub fn current() -> Result<Self, IpcError> {
        process_token()
            .and_then(|token| Self::from_token(&token))
            .map_err(|e| IpcError::io("Could not read your user account", e))
    }

    /// The account `process` runs as. The handle needs
    /// `PROCESS_QUERY_LIMITED_INFORMATION`.
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when the process's token cannot be opened or read, as for
    /// most processes of another user.
    pub fn of(process: BorrowedHandle<'_>) -> Result<Self, IpcError> {
        token_of(process)
            .and_then(|token| Self::from_token(&token))
            .map_err(|e| IpcError::io("Could not read the account of another program", e))
    }

    fn from_token(token: &OwnedHandle) -> io::Result<Self> {
        let user = token_sid(token, TokenUser)?;
        let integrity = token_sid(token, TokenIntegrityLevel)?
            .integrity_level()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Windows reported an integrity level that is not one",
                )
            })?;
        Ok(Self { user, integrity })
    }
}

/// The SID that a token information class names in `token`, which must be open for
/// `TOKEN_QUERY`. `TokenUser` gives a `TOKEN_USER` and `TokenIntegrityLevel` a `TOKEN_MANDATORY_LABEL`; both are a single `SID_AND_ATTRIBUTES`, so one reader serves both.
fn token_sid(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> io::Result<Sid> {
    debug_assert!(class == TokenUser || class == TokenIntegrityLevel);
    let handle = token.as_raw_handle() as HANDLE;

    let mut needed = 0u32;
    // SAFETY: a size query with a null buffer; it fails by design and reports the
    // size in `needed`.
    unsafe { GetTokenInformation(handle, class, ptr::null_mut(), 0, &raw mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 elements give the 8-byte alignment SID_AND_ATTRIBUTES (which holds a
    // pointer) needs. The buffer is sized from `capacity`, so the size passed
    // below is exactly what it holds.
    let capacity = needed
        .checked_next_multiple_of(8)
        .ok_or_else(|| io::Error::other("the token information is too large"))?;
    let mut buf = vec![0u64; capacity as usize / 8];
    // SAFETY: `buf` is writable for `capacity` bytes.
    if unsafe {
        GetTokenInformation(
            handle,
            class,
            buf.as_mut_ptr().cast(),
            capacity,
            &raw mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: for both classes the call above filled `buf` with a structure whose
    // only field is a SID_AND_ATTRIBUTES, and `buf` is aligned for it. Its SID
    // pointer points inside `buf`, which outlives the copy.
    let entry = unsafe { &*buf.as_ptr().cast::<SID_AND_ATTRIBUTES>() };
    // SAFETY: see above.
    unsafe { Sid::copy_from(entry.Sid) }
}

/// The token the client at the other end of `pipe` connected with, for reading only.
///
/// Reads it by impersonating the client for the length of one call on this thread
/// and reverting before returning, with no `.await` in between, so nothing else
/// ever runs on the thread as the client.
///
/// A client hides its token two ways: by opening the pipe at the anonymous level,
/// which fails here with `ERROR_CANT_OPEN_ANONYMOUS`, or by opening it without a
/// security quality of service, whose token Windows hands over only once the client
/// has written (`ERROR_CANNOT_IMPERSONATE` until then). The connector names the
/// identification level when it opens the pipe, so its token is there at once.
fn client_token(pipe: &NamedPipeServer) -> io::Result<OwnedHandle> {
    // SAFETY: `pipe` is a connected server instance whose handle stays open for the
    // call; on success this thread impersonates the client until the guard reverts.
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle() as HANDLE) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let impersonating = Impersonating;
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentThread returns a pseudo handle that needs no closing; on
    // success `raw` is a new token handle that we own. `OpenAsSelf` checks the
    // access against this process's own token, not the client's.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, TRUE, &raw mut raw) };
    // Read before reverting, which may set the thread's last error again.
    let error = io::Error::last_os_error();
    drop(impersonating);
    if opened == 0 {
        return Err(error);
    }
    // SAFETY: `raw` is a valid handle we own and nothing else closes it.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) })
}

/// Ends the current thread's impersonation when dropped.
struct Impersonating;

impl Drop for Impersonating {
    fn drop(&mut self) {
        // SAFETY: a plain call with no arguments; it ends the impersonation this thread started before making the guard.
        if unsafe { RevertToSelf() } == 0 {
            // The thread would go on running as the client, and every task the
            // runtime puts on it with it. Nothing may run that way.
            std::process::abort();
        }
    }
}

/// Who a client is: the user and the integrity level of the token it connected with.
#[derive(Debug)]
struct ClientIdentity {
    user: Sid,
    integrity: Sid,
    /// The token is restricted (its access is also checked against restricting SIDs) or an
    /// `AppContainer`'s: a sandbox, whatever its user and level.
    sandboxed: bool,
}

impl ClientIdentity {
    fn of(pipe: &NamedPipeServer) -> io::Result<Self> {
        let token = client_token(pipe)?;
        let mut app_container = 0u32;
        // SAFETY: TokenIsAppContainer fills a DWORD.
        unsafe { read_token_value(&token, TokenIsAppContainer, &mut app_container) }?;
        // SAFETY: `token` is open for `TOKEN_QUERY` for the duration of the call.
        let restricted = unsafe { IsTokenRestricted(token.as_raw_handle() as HANDLE) } != 0;
        Ok(Self {
            user: token_sid(&token, TokenUser)?,
            integrity: token_sid(&token, TokenIntegrityLevel)?,
            sandboxed: restricted || app_container != 0,
        })
    }
}

/// Serves a client only when it is `user` at `integrity` or above, outside any sandbox,
/// which is who the pipe's descriptor lets in; this check holds even if the descriptor does
/// not.
///
/// A client must open the pipe at the identification level, as [`connect`] does. One that
/// opens without saying at which level it may be identified (plain `CreateFile`, .NET's
/// `NamedPipeClientStream` by default) hands the server its token only once it has
/// written, and serving it would mean reading from a client nobody has checked, so it is
/// refused with a message that says how to open the pipe.
fn check_client(
    user: &Sid,
    integrity: &Sid,
    client: io::Result<ClientIdentity>,
) -> Result<(), IpcError> {
    let refused = |peer: String| Err(IpcError::PeerRefused { peer });
    let client = match client {
        Ok(client) => client,
        Err(e) if is_os_error(&e, ERROR_CANT_OPEN_ANONYMOUS) => {
            return refused(
                "a client that opened the connection anonymously, so it would not say who it is"
                    .into(),
            );
        }
        Err(e) if is_os_error(&e, ERROR_CANNOT_IMPERSONATE) => {
            return refused(
                "a client that did not say at which level it may be identified (open the connection at the identification level, as wirecatch_ipc::connect does)"
                    .into(),
            );
        }
        Err(e) => return Err(IpcError::io("Could not check who is connecting", e)),
    };
    if &client.user != user {
        return refused(format!("another user account ({})", client.user));
    }
    let at_least_ours = match (
        client.integrity.integrity_level(),
        integrity.integrity_level(),
    ) {
        (Some(theirs), Some(ours)) => theirs >= ours,
        _ => false,
    };
    if !at_least_ours {
        return refused(format!(
            "a program of yours running with fewer rights than Wirecatch (integrity level {})",
            client.integrity
        ));
    }
    if client.sandboxed {
        return refused("a program of yours running in a sandbox".into());
    }
    Ok(())
}

/// How the current process holds administrator rights.
pub(crate) fn elevation() -> io::Result<Elevation> {
    let token = process_token()?;
    let mut kind: TOKEN_ELEVATION_TYPE = 0;
    // SAFETY: TokenElevationType fills a TOKEN_ELEVATION_TYPE.
    unsafe { read_token_value(&token, TokenElevationType, &mut kind) }?;
    let mut elevated = TOKEN_ELEVATION { TokenIsElevated: 0 };
    // SAFETY: TokenElevation fills a TOKEN_ELEVATION.
    unsafe { read_token_value(&token, TokenElevation, &mut elevated) }?;
    classify_elevation(kind, elevated.TokenIsElevated != 0)
}

/// Tells an elevation the person chose apart from an account that is always elevated. `TokenIsElevated` alone is set in both cases; only a token that user account control split in two has a full and a limited half.
fn classify_elevation(kind: TOKEN_ELEVATION_TYPE, elevated: bool) -> io::Result<Elevation> {
    match kind {
        FULL_TOKEN => Ok(Elevation::Elevated),
        SINGLE_TOKEN if elevated => Ok(Elevation::AlwaysAdministrator),
        LIMITED_TOKEN | SINGLE_TOKEN => Ok(Elevation::Standard),
        other => Err(io::Error::other(format!(
            "Windows reported an unknown elevation type {other}"
        ))),
    }
}

/// Reads a fixed-size token information class into `out`.
///
/// # Safety
///
/// `T` must be the structure Windows fills for `class`.
unsafe fn read_token_value<T>(
    token: &OwnedHandle,
    class: TOKEN_INFORMATION_CLASS,
    out: &mut T,
) -> io::Result<()> {
    let size = win32_size_of::<T>();
    let mut returned = 0u32;
    // SAFETY: `out` is writable for `size` bytes, and the caller guarantees it is the
    // structure `class` fills.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle() as HANDLE,
            class,
            ptr::from_mut(out).cast(),
            size,
            &raw mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Frees a block that Windows allocated with `LocalAlloc`.
struct LocalBox(*mut c_void);

impl Drop for LocalBox {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from a Windows API documented to allocate
            // with LocalAlloc, and is freed exactly once.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// `text` as NUL-terminated UTF-16, the form Windows calls take.
pub fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(iter::once(0)).collect()
}

/// The SDDL of an object that is `sid`'s alone:
///
/// - `O:` owned by `sid`;
/// - `D:P(A;;FA;;;sid)`: a protected DACL (nothing inherited) that grants full file access to `sid` and nothing to anyone else.
fn private_sddl(sid: &str) -> String {
    format!("O:{sid}D:P(A;;FA;;;{sid})")
}

/// The SDDL for the pipe: [`private_sddl`], and `S:(ML;;NWNR;;;integrity)`: a mandatory label at the server's own integrity level with no-write-up and no-read-up, so only a process at that level or above can open it, even though every process of the user matches the DACL. A normal server is medium, which keeps out low-integrity processes (a sandboxed renderer, for example); an elevated one is high, which keeps out every unelevated process, so the pipe can never be a way around UAC. Without a label Windows applies medium no-write-up only, which still lets lower processes read.
pub(crate) fn owner_only_sddl(sid: &str, integrity: &str) -> String {
    format!("{}S:(ML;;NWNR;;;{integrity})", private_sddl(sid))
}

/// A self-relative security descriptor, normally built from [`owner_only_sddl`] or [`private_sddl`].
struct OwnerOnlyDescriptor(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is a heap block owned by this value and never written after creation; Windows only reads it when creating pipe instances and files.
unsafe impl Send for OwnerOnlyDescriptor {}
// SAFETY: as above, shared access is read-only.
unsafe impl Sync for OwnerOnlyDescriptor {}

impl OwnerOnlyDescriptor {
    fn new(user: &Sid, integrity: &Sid) -> io::Result<Self> {
        Self::from_sddl(&owner_only_sddl(
            &user.to_string_form()?,
            &integrity.to_string_form()?,
        ))
    }

    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let wide = wide(OsStr::new(sddl));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; on success `descriptor` receives a
        // LocalAlloc block that Drop frees.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }
}

impl Drop for OwnerOnlyDescriptor {
    fn drop(&mut self) {
        drop(LocalBox(self.0));
    }
}

impl fmt::Debug for OwnerOnlyDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OwnerOnlyDescriptor")
    }
}

/// Creates one pipe instance with the owner-only descriptor.
fn create_instance(
    endpoint: &Endpoint,
    descriptor: &OwnerOnlyDescriptor,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: win32_size_of::<SECURITY_ATTRIBUTES>(),
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .pipe_mode(PipeMode::Byte);
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor is
    // alive for the whole call (it is owned by `descriptor`, borrowed here).
    unsafe {
        options
            .create_with_security_attributes_raw(endpoint.as_os_str(), (&raw mut attributes).cast())
    }
}

/// Opens an existing pipe as a client just long enough to read its owner. Never
/// writes to it.
fn probe_owner(endpoint: &Endpoint) -> io::Result<Sid> {
    let client = ClientOptions::new()
        .security_qos_flags(SECURITY_IDENTIFICATION)
        .open(endpoint.as_os_str())?;
    handle_owner(client.as_raw_handle())
}

/// Decides what a pipe name that already exists means, from what probing it
/// found.
fn classify_taken(endpoint: &Endpoint, user: &Sid, probe: io::Result<Sid>) -> IpcError {
    let taken_by = |owner: String| IpcError::EndpointTaken {
        endpoint: endpoint.to_string(),
        owner,
    };
    match probe {
        Ok(owner) if &owner == user => IpcError::AlreadyInUse {
            endpoint: endpoint.to_string(),
        },
        Ok(owner) => taken_by(owner.to_string()),
        // Our own pipe always lets us in, so a refusal means someone else's.
        Err(e) if is_os_error(&e, ERROR_ACCESS_DENIED) => {
            taken_by("an account that does not let you in".into())
        }
        // Every instance is busy serving a client: our own app under load is the
        // likely case, and either way the name is not ours to take.
        Err(e) if is_os_error(&e, ERROR_PIPE_BUSY) => IpcError::AlreadyInUse {
            endpoint: endpoint.to_string(),
        },
        Err(e) => IpcError::io(
            format!("{endpoint} is already in use and could not be checked"),
            e,
        ),
    }
}

/// The server side. Always keeps one instance waiting for the next client.
#[derive(Debug)]
pub(crate) struct PipeListener {
    endpoint: Endpoint,
    descriptor: OwnerOnlyDescriptor,
    /// Who may connect: this user, at this integrity level or above.
    user: Sid,
    integrity: Sid,
    next: NamedPipeServer,
}

impl PipeListener {
    pub(crate) fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        Self::bind_labelled(endpoint, &Sid::current_integrity()?)
    }

    /// Listens with the pipe labelled at `integrity`, which is always the process's own outside tests.
    fn bind_labelled(endpoint: &Endpoint, integrity: &Sid) -> Result<Self, IpcError> {
        let user = Sid::current_user()?;
        let descriptor = OwnerOnlyDescriptor::new(&user, integrity).map_err(|e| {
            IpcError::io("Could not prepare the access rules for the connection", e)
        })?;
        Self::bind_with(endpoint, user, integrity.clone(), descriptor)
    }

    /// Listens behind `sddl` instead of the owner-only descriptor: a stand-in for a
    /// descriptor that regressed, to show what the listener's own check still refuses.
    #[cfg(test)]
    fn bind_behind(endpoint: &Endpoint, sddl: &str) -> Result<Self, IpcError> {
        let descriptor = OwnerOnlyDescriptor::from_sddl(sddl).unwrap();
        Self::bind_with(
            endpoint,
            Sid::current_user()?,
            Sid::current_integrity()?,
            descriptor,
        )
    }

    fn bind_with(
        endpoint: &Endpoint,
        user: Sid,
        integrity: Sid,
        descriptor: OwnerOnlyDescriptor,
    ) -> Result<Self, IpcError> {
        let next = match create_instance(endpoint, &descriptor, true) {
            Ok(next) => next,
            // FILE_FLAG_FIRST_PIPE_INSTANCE fails this way when the name already
            // exists, whoever created it. Find out who, so the user gets the
            // right advice.
            Err(e) if is_os_error(&e, ERROR_ACCESS_DENIED) => {
                return Err(classify_taken(endpoint, &user, probe_owner(endpoint)));
            }
            Err(e) => return Err(IpcError::io(format!("Could not listen at {endpoint}"), e)),
        };
        Ok(Self {
            endpoint: endpoint.clone(),
            descriptor,
            user,
            integrity,
            next,
        })
    }

    pub(crate) async fn accept(&mut self) -> Result<Connection, IpcError> {
        if let Err(e) = self.next.connect().await {
            // The instance may be unusable after a failed connect; start the next
            // attempt on a fresh one.
            self.next = self.new_instance()?;
            return Err(IpcError::io("A client failed to connect", e));
        }
        let fresh = self.new_instance()?;
        let connected = mem::replace(&mut self.next, fresh);
        // Dropping a refused instance disconnects its client before anything is read
        // or written.
        check_client(&self.user, &self.integrity, ClientIdentity::of(&connected))?;
        Ok(Connection {
            inner: Inner::PipeServer(connected),
        })
    }

    fn new_instance(&self) -> Result<NamedPipeServer, IpcError> {
        create_instance(&self.endpoint, &self.descriptor, false)
            .map_err(|e| IpcError::io(format!("Could not keep listening at {}", self.endpoint), e))
    }

    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    #[cfg(test)]
    pub(crate) fn waiting_instance(&self) -> RawHandle {
        self.next.as_raw_handle()
    }
}

/// Opens the pipe and refuses it unless `expected` owns it. Nothing is written
/// before the check passes.
pub(crate) async fn connect(endpoint: &Endpoint, expected: &Sid) -> Result<Connection, IpcError> {
    let started = Instant::now();
    let client = loop {
        // Identification level: the server may learn who we are but cannot act as
        // us.
        let opened = ClientOptions::new()
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(endpoint.as_os_str());
        match opened {
            Ok(client) => break client,
            Err(e) if is_os_error(&e, ERROR_PIPE_BUSY) && started.elapsed() < BUSY_WAIT => {
                // No completion event exists for "an instance became free"
                // (WaitNamedPipe blocks a thread), so wait briefly and retry.
                sleep(BUSY_RETRY).await;
            }
            Err(e) if is_os_error(&e, ERROR_FILE_NOT_FOUND) => {
                return Err(IpcError::NotRunning {
                    endpoint: endpoint.to_string(),
                });
            }
            Err(e) => {
                return Err(IpcError::io(
                    format!("Could not open the connection at {endpoint}"),
                    e,
                ));
            }
        }
    };
    let owner = handle_owner(client.as_raw_handle())
        .map_err(|e| IpcError::io("Could not check who owns the connection", e))?;
    verify_owner(endpoint, expected, &owner)?;
    Ok(Connection {
        inner: Inner::PipeClient(client),
    })
}

/// Refuses unless `actual` is `expected`.
pub(crate) fn verify_owner(
    endpoint: &Endpoint,
    expected: &Sid,
    actual: &Sid,
) -> Result<(), IpcError> {
    if expected == actual {
        Ok(())
    } else {
        Err(IpcError::WrongOwner {
            endpoint: endpoint.to_string(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        })
    }
}

/// The owner of an open file.
pub(crate) fn file_owner(file: &std::fs::File) -> io::Result<Sid> {
    handle_owner(file.as_raw_handle())
}

/// Whether a file that `owner` owns may be taken as `user`'s own: the user's, or Administrators' or the system's, which own what an elevated program of the user (or any program of an administrator with UAC off) creates without naming an owner. Only a token that holds one of those as an owner can give it a file, and such a token can already read and write all of the user's files (D41.4).
pub(crate) fn may_own_users_file(owner: &Sid, user: &Sid) -> bool {
    owner == user || *owner == Sid::administrators() || *owner == Sid::local_system()
}

/// Creates a new file at `path`, open for writing, that is the user's alone: owned by the user and open to nobody else ([`private_sddl`]). Without an owner of its own the file would get the program's default owner, Administrators in an elevated program.
///
/// # Errors
///
/// The file exists already (`AlreadyExists`), or Windows refuses to create it.
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let user = process_token().and_then(|token| token_sid(&token, TokenUser))?;
    let descriptor = OwnerOnlyDescriptor::from_sddl(&private_sddl(&user.to_string_form()?))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: win32_size_of::<SECURITY_ATTRIBUTES>(),
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let wide = wide(path.as_os_str());
    // SAFETY: `wide` is NUL-terminated; `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor `descriptor` keeps alive for the whole call; there is no template file. On success the handle is new and ours.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            &raw const attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid file handle that nothing else owns or closes.
    Ok(unsafe { File::from_raw_handle(handle as RawHandle) })
}

/// Reads the owner of a kernel object from its handle (needs `READ_CONTROL`, which
/// a pipe client opened for reading has).
fn handle_owner(handle: RawHandle) -> io::Result<Sid> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `handle` is open for the duration of the call; on success
    // `descriptor` is a LocalAlloc block that the guard frees, and `owner` points
    // inside it.
    let status = unsafe {
        GetSecurityInfo(
            handle as HANDLE,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &raw mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(os_error(status));
    }
    let _guard = LocalBox(descriptor);
    // SAFETY: `owner` points into `descriptor`, which is alive until the guard
    // drops after this copy.
    unsafe { Sid::copy_from(owner) }
}

/// Runs `work` on a thread of its own whose token makes Administrators the owner of what it creates, as an elevated program's token does unless the computer is set otherwise. `None` when this program's token cannot do that (it is not elevated), for the caller to say that it checked nothing.
#[cfg(test)]
pub(crate) fn as_an_elevated_program<T: Send>(work: impl FnOnce() -> T + Send) -> Option<T> {
    use windows_sys::Win32::Foundation::ERROR_INVALID_OWNER;
    use windows_sys::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
        TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_OWNER, TokenImpersonation, TokenOwner,
    };
    use windows_sys::Win32::System::Threading::SetThreadToken;

    let administrators = Sid::administrators();
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let mut raw: HANDLE = ptr::null_mut();
                // SAFETY: the pseudo handle of the current process needs no closing; on success `raw` is a new token handle that we own.
                let opened =
                    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE, &raw mut raw) };
                assert_ne!(opened, 0, "{}", io::Error::last_os_error());
                // SAFETY: `raw` is the open token handle from above, owned only here.
                let process = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
                let mut raw: HANDLE = ptr::null_mut();
                // SAFETY: `process` is open with TOKEN_DUPLICATE; on success `raw` is a new impersonation token that we own.
                let duplicated = unsafe {
                    DuplicateTokenEx(
                        process.as_raw_handle() as HANDLE,
                        TOKEN_IMPERSONATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
                        ptr::null(),
                        SecurityImpersonation,
                        TokenImpersonation,
                        &raw mut raw,
                    )
                };
                assert_ne!(duplicated, 0, "{}", io::Error::last_os_error());
                // SAFETY: `raw` is the new token handle from above, owned only here.
                let token = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
                let owner = TOKEN_OWNER {
                    Owner: administrators.as_psid(),
                };
                // SAFETY: `token` is open with TOKEN_ADJUST_DEFAULT; `owner` is a TOKEN_OWNER of the size given, and the SID it points at outlives the call.
                let set = unsafe {
                    SetTokenInformation(
                        token.as_raw_handle() as HANDLE,
                        TokenOwner,
                        (&raw const owner).cast(),
                        win32_size_of::<TOKEN_OWNER>(),
                    )
                };
                if set == 0 {
                    let error = io::Error::last_os_error();
                    assert!(is_os_error(&error, ERROR_INVALID_OWNER), "{error}");
                    return None;
                }
                // SAFETY: a null thread means this thread, and `token` is an impersonation token; the impersonation is reverted below, before the thread ends.
                let impersonating =
                    unsafe { SetThreadToken(ptr::null(), token.as_raw_handle() as HANDLE) };
                assert_ne!(impersonating, 0, "{}", io::Error::last_os_error());
                let _revert = Impersonating;
                Some(work())
            })
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, DuplicateTokenEx, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, ImpersonateAnonymousToken, LABEL_SECURITY_INFORMATION,
        RevertToSelf, SE_DACL_PROTECTED, SID_AND_ATTRIBUTES, SecurityImpersonation,
        SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_DUPLICATE, TOKEN_IMPERSONATE,
        TOKEN_MANDATORY_LABEL, TokenImpersonation, TokenIntegrityLevel,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ALL_ACCESS, OPEN_EXISTING, SECURITY_SQOS_PRESENT,
    };
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, SE_GROUP_INTEGRITY,
        SYSTEM_MANDATORY_LABEL_ACE_TYPE, SYSTEM_MANDATORY_LABEL_NO_READ_UP,
        SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadToken};

    /// A pipe name no other test uses: the one test-support gives every test endpoint. A pipe needs no folder, so the folder goes at once.
    fn unique_endpoint() -> Endpoint {
        Endpoint::from_raw(
            wirecatch_test_support::private_dir("ipc-pipe")
                .unwrap()
                .endpoint_name(),
        )
    }

    #[derive(Debug, PartialEq, Eq)]
    struct AceInfo {
        ace_type: u32,
        mask: u32,
        sid: Sid,
    }

    #[derive(Debug)]
    struct Security {
        owner: Sid,
        dacl_protected: bool,
        /// `None` is a null DACL, which would mean everyone has full access.
        aces: Option<Vec<AceInfo>>,
        /// Mandatory label entries from the SACL.
        label: Vec<AceInfo>,
    }

    /// Reads the entries of an ACL whose entries all share the
    /// `ACCESS_ALLOWED_ACE` layout (allowed, denied and mandatory label entries).
    ///
    /// # Safety
    ///
    /// `acl` must point at a valid ACL that stays alive during the call.
    unsafe fn read_acl(acl: *const ACL, expected: &[u32]) -> io::Result<Vec<AceInfo>> {
        // SAFETY: zeroed is a valid ACL_SIZE_INFORMATION (plain integers).
        let mut info: ACL_SIZE_INFORMATION = unsafe { mem::zeroed() };
        // SAFETY: `acl` is valid (caller); `info` matches the requested class.
        if unsafe {
            GetAclInformation(
                acl,
                (&raw mut info).cast(),
                win32_size_of::<ACL_SIZE_INFORMATION>(),
                AclSizeInformation,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut aces = Vec::new();
        for index in 0..info.AceCount {
            let mut ace: *mut c_void = ptr::null_mut();
            // SAFETY: `index` is below the ACE count reported above.
            if unsafe { GetAce(acl, index, &raw mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { &*(ace as *const ACE_HEADER) };
            let ace_type = u32::from(header.AceType);
            if !expected.contains(&ace_type) {
                return Err(io::Error::other(format!("unexpected ACE type {ace_type}")));
            }
            // SAFETY: the expected types share the ACCESS_ALLOWED_ACE layout, with
            // the SID starting at `SidStart`.
            let body = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
            // SAFETY: as above.
            let sid = unsafe { Sid::copy_from(ptr::addr_of!(body.SidStart) as PSID) }?;
            aces.push(AceInfo {
                ace_type,
                mask: body.Mask,
                sid,
            });
        }
        Ok(aces)
    }

    /// Reads the owner, DACL and mandatory label of an object back from
    /// Windows.
    fn read_security(handle: RawHandle) -> io::Result<Security> {
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut sacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: as in `handle_owner`. Reading the label needs only READ_CONTROL.
        let status = unsafe {
            GetSecurityInfo(
                handle as HANDLE,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                &raw mut owner,
                ptr::null_mut(),
                &raw mut dacl,
                &raw mut sacl,
                &raw mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(os_error(status));
        }
        let _guard = LocalBox(descriptor);
        // SAFETY: `owner` points into the live descriptor.
        let owner = unsafe { Sid::copy_from(owner) }?;

        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `descriptor` is valid until the guard drops.
        if unsafe { GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let aces = if dacl.is_null() {
            None
        } else {
            // SAFETY: `dacl` points into the live descriptor.
            Some(unsafe { read_acl(dacl, &[ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE]) }?)
        };
        let label = if sacl.is_null() {
            Vec::new()
        } else {
            // SAFETY: `sacl` points into the live descriptor.
            unsafe { read_acl(sacl, &[SYSTEM_MANDATORY_LABEL_ACE_TYPE]) }?
        };
        Ok(Security {
            owner,
            dacl_protected: control & SE_DACL_PROTECTED != 0,
            aces,
            label,
        })
    }

    fn assert_owner_only(security: &Security, user: &Sid) {
        assert_eq!(&security.owner, user, "owner");
        assert!(security.dacl_protected, "DACL must not inherit entries");
        assert_eq!(
            security.aces.as_deref(),
            Some(
                &[AceInfo {
                    ace_type: ACCESS_ALLOWED_ACE_TYPE,
                    mask: FILE_ALL_ACCESS,
                    sid: user.clone(),
                }][..]
            ),
            "the only entry must grant the current user, and nobody else"
        );
        assert_eq!(
            security.label,
            [AceInfo {
                ace_type: SYSTEM_MANDATORY_LABEL_ACE_TYPE,
                mask: SYSTEM_MANDATORY_LABEL_NO_WRITE_UP | SYSTEM_MANDATORY_LABEL_NO_READ_UP,
                // The server's own integrity: medium normally, high when the tests run elevated.
                sid: Sid::current_integrity().unwrap(),
            }],
            "a label at the server's own integrity, with no-write-up and no-read-up"
        );
    }

    #[test]
    fn endpoint_is_named_after_the_current_user_and_the_key() {
        let user = Sid::current_user().unwrap().to_string_form().unwrap();
        assert!(user.starts_with("S-1-5-"), "{user}");
        let key = EndpointKey::parse("0123456789abcdef0123456789abcdef").unwrap();
        let named = endpoint(Channel::Release, None, || Ok(key.clone())).unwrap();
        assert_eq!(
            named.as_os_str(),
            OsStr::new(&format!(
                "{}{}{user}-{}",
                paths::PIPE_ROOT,
                Channel::Release.pipe_prefix(),
                key.as_str()
            ))
        );
    }

    // The pipe used to be named after the SID alone, which every account on the computer
    // can read, so any of them could create it first and keep Wirecatch from starting
    // (IPB-10). Built with a made-up SID, so the test never touches a real Wirecatch's name.
    #[test]
    fn a_keyed_pipe_name_cannot_be_worked_out_from_the_users_sid() {
        const SID: &str = "S-1-5-21-1111111111-2222222222-3333333333-1001";
        let derivable = format!(
            "{}{}{SID}",
            paths::PIPE_ROOT,
            Channel::Release.pipe_prefix()
        );
        let one = EndpointKey::parse("0123456789abcdef0123456789abcdef").unwrap();
        let other = EndpointKey::parse("fedcba9876543210fedcba9876543210").unwrap();
        let keyed = paths::pipe_name(Channel::Release, SID, one.as_str());
        assert_eq!(keyed, format!("{derivable}-{}", one.as_str()));
        assert_ne!(
            keyed,
            paths::pipe_name(Channel::Release, SID, other.as_str())
        );
    }

    #[test]
    fn the_current_account_is_the_current_user_at_the_current_level() {
        let account = ProcessAccount::current().unwrap();
        assert_eq!(account.user, Sid::current_user().unwrap());
        assert_eq!(
            Some(account.integrity),
            Sid::current_integrity().unwrap().integrity_level()
        );
    }

    // Another account that once saw the keyed name (every account can list pipe names) can
    // take it while Wirecatch is closed; the listener then writes a new key instead of
    // staying shut out (D41.6).
    #[tokio::test]
    async fn a_listener_whose_keyed_name_is_taken_listens_under_a_new_key() {
        let dir = wirecatch_test_support::private_dir("ipc-keyed-taken").unwrap();
        let folders = crate::Folders {
            key: dir.path().to_path_buf(),
            runtime: None,
        };
        let first = crate::Listener::bind_keyed(Channel::Dev, &folders).unwrap();
        let seen = first.endpoint().clone();
        drop(first);

        let user = Sid::current_user().unwrap();
        let foreign = OwnerOnlyDescriptor::from_sddl(&format!(
            "O:{}D:P(A;;FA;;;SY)",
            user.to_string_form().unwrap()
        ))
        .unwrap();
        let _squatter = create_instance(&seen, &foreign, true).unwrap();

        let mut listener = crate::Listener::bind_keyed(Channel::Dev, &folders).unwrap();
        assert_ne!(listener.endpoint(), &seen);
        let current = crate::endpoint_in(Channel::Dev, &folders).unwrap();
        assert_eq!(&current, listener.endpoint(), "clients read the new key");
        let (client, server) = tokio::join!(connect(&current, &user), listener.accept());
        client.unwrap();
        server.unwrap();
    }

    #[test]
    fn sid_round_trips_through_its_string_form() {
        let system = Sid::parse("S-1-5-18").unwrap();
        assert_eq!(system.to_string_form().unwrap(), "S-1-5-18");
        assert!(Sid::parse("not a sid").is_err());
    }

    #[test]
    fn the_well_known_accounts_have_their_well_known_ids() {
        assert_eq!(Sid::administrators(), Sid::parse("S-1-5-32-544").unwrap());
        assert_eq!(Sid::local_system(), Sid::parse("S-1-5-18").unwrap());
    }

    // An elevated program, and every program of an administrator with UAC off, gives what it creates to Administrators unless it names an owner, so a key such a Wirecatch wrote was refused by every later run, its own included (W4I review R2-H1).
    #[test]
    fn only_the_user_administrators_or_the_system_may_own_the_users_files() {
        let user = Sid::current_user().unwrap();
        assert!(may_own_users_file(&user, &user));
        assert!(may_own_users_file(&Sid::administrators(), &user));
        assert!(may_own_users_file(&Sid::local_system(), &user));
        for other in [
            "S-1-5-21-1-2-3-1002",
            "S-1-5-32-545",
            "S-1-5-32-546",
            "S-1-1-0",
            "S-1-5-11",
            "S-1-5-7",
            "S-1-5-19",
            "S-1-5-20",
            "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
            "S-1-16-12288",
        ] {
            let other = Sid::parse(other).unwrap();
            assert!(!may_own_users_file(&other, &user), "{other}");
        }
    }

    /// The owner and the access entries of the endpoint key a listener writes in a new folder.
    fn security_of_a_new_key() -> Security {
        let dir = wirecatch_test_support::private_dir("ipc-key-security").unwrap();
        let lock = crate::key::KeyLock::take(dir.path()).unwrap();
        lock.current().unwrap();
        let file = std::fs::File::open(dir.path().join(crate::key::KEY_FILE)).unwrap();
        read_security(file.as_raw_handle()).unwrap()
    }

    fn assert_the_users_alone(security: &Security, user: &Sid) {
        assert_eq!(&security.owner, user, "owner");
        assert!(security.dacl_protected, "the DACL must not inherit entries");
        assert_eq!(
            security.aces.as_deref(),
            Some(
                &[AceInfo {
                    ace_type: ACCESS_ALLOWED_ACE_TYPE,
                    mask: FILE_ALL_ACCESS,
                    sid: user.clone(),
                }][..]
            ),
            "the only entry must grant the current user, and nobody else"
        );
    }

    // The key took the folder's entries and the program's default owner, which an elevated program sets to Administrators (W4I review R2-H1).
    #[test]
    fn the_endpoint_key_is_the_users_alone_even_from_an_elevated_listener() {
        let user = Sid::current_user().unwrap();
        assert_the_users_alone(&security_of_a_new_key(), &user);
        match as_an_elevated_program(security_of_a_new_key) {
            Some(security) => assert_the_users_alone(&security, &user),
            None => eprintln!(
                "this program is not elevated, so it cannot make Administrators the owner of what it creates: the elevated case was not checked"
            ),
        }
    }

    #[tokio::test]
    async fn listener_pipe_has_the_owner_only_descriptor() {
        let user = Sid::current_user().unwrap();
        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind(&endpoint).unwrap();
        assert_owner_only(&read_security(listener.waiting_instance()).unwrap(), &user);

        // The descriptor also holds for later instances and is what a client sees.
        let client_side = connect(&endpoint, &user).await.unwrap();
        let server_side = listener.accept().await.unwrap();
        assert_owner_only(&read_security(listener.waiting_instance()).unwrap(), &user);
        let Inner::PipeClient(client) = &client_side.inner else {
            panic!("expected a client")
        };
        assert_owner_only(&read_security(client.as_raw_handle()).unwrap(), &user);
        drop(server_side);
    }

    /// Opens the pipe from a thread impersonating the anonymous logon, a
    /// different security principal than the current user, and returns the open
    /// client end. Opens at the identification level, as the connector does, so the
    /// server can tell who it is.
    fn open_as_anonymous(endpoint: &Endpoint, access: u32) -> Result<OwnedHandle, u32> {
        let name = wide(endpoint.as_os_str());
        std::thread::spawn(move || {
            // SAFETY: impersonation only affects this short-lived thread and is
            // reverted before it ends.
            assert_ne!(unsafe { ImpersonateAnonymousToken(GetCurrentThread()) }, 0);
            // SAFETY: `name` is NUL-terminated; the handle is closed below.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    access,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                )
            };
            // SAFETY: plain thread-local error query.
            let error = unsafe { GetLastError() };
            // SAFETY: undoes the impersonation above.
            unsafe { RevertToSelf() };
            if handle == INVALID_HANDLE_VALUE {
                Err(error)
            } else {
                // SAFETY: `handle` is a valid handle we own and nothing else closes.
                Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
            }
        })
        .join()
        .unwrap()
    }

    #[tokio::test]
    async fn another_principal_cannot_open_the_pipe() {
        // Control: the default pipe DACL lets the anonymous logon read, which is
        // exactly why the listener sets its own.
        let control = unique_endpoint();
        let _default_pipe = ServerOptions::new()
            .first_pipe_instance(true)
            .create(control.as_os_str())
            .unwrap();
        assert_eq!(open_as_anonymous(&control, GENERIC_READ).map(drop), Ok(()));

        let endpoint = unique_endpoint();
        let _listener = PipeListener::bind(&endpoint).unwrap();
        assert_eq!(
            open_as_anonymous(&endpoint, GENERIC_READ).map(drop),
            Err(ERROR_ACCESS_DENIED)
        );
        assert_eq!(
            open_as_anonymous(&endpoint, GENERIC_READ | GENERIC_WRITE).map(drop),
            Err(ERROR_ACCESS_DENIED)
        );
    }

    /// Opens the pipe from a thread running as the current user but at low
    /// integrity, like a sandboxed browser process, and returns the open client end.
    /// Opens at the identification level, as the connector does.
    fn open_at_low_integrity(endpoint: &Endpoint, access: u32) -> Result<OwnedHandle, u32> {
        let name = wide(endpoint.as_os_str());
        std::thread::spawn(move || {
            let low = Sid::parse("S-1-16-4096").unwrap();
            let mut process_token: HANDLE = ptr::null_mut();
            let mut token: HANDLE = ptr::null_mut();
            // SAFETY: plain Win32 calls on handles opened and closed here; the
            // impersonation only affects this thread and is reverted.
            unsafe {
                assert_ne!(
                    OpenProcessToken(
                        GetCurrentProcess(),
                        TOKEN_DUPLICATE | TOKEN_QUERY,
                        &raw mut process_token
                    ),
                    0
                );
                assert_ne!(
                    DuplicateTokenEx(
                        process_token,
                        TOKEN_IMPERSONATE | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
                        ptr::null(),
                        SecurityImpersonation,
                        TokenImpersonation,
                        &raw mut token,
                    ),
                    0
                );
                CloseHandle(process_token);
                let label = TOKEN_MANDATORY_LABEL {
                    Label: SID_AND_ATTRIBUTES {
                        Sid: low.as_psid(),
                        Attributes: SE_GROUP_INTEGRITY as u32,
                    },
                };
                let size = win32_size_of::<TOKEN_MANDATORY_LABEL>() + GetLengthSid(low.as_psid());
                assert_ne!(
                    SetTokenInformation(
                        token,
                        TokenIntegrityLevel,
                        (&raw const label).cast(),
                        size,
                    ),
                    0
                );
                assert_ne!(SetThreadToken(ptr::null(), token), 0);
                let handle = CreateFileW(
                    name.as_ptr(),
                    access,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                );
                let error = GetLastError();
                RevertToSelf();
                CloseHandle(token);
                if handle == INVALID_HANDLE_VALUE {
                    Err(error)
                } else {
                    Ok(OwnedHandle::from_raw_handle(handle as RawHandle))
                }
            }
        })
        .join()
        .unwrap()
    }

    #[tokio::test]
    async fn a_low_integrity_process_of_the_same_user_cannot_open_the_pipe() {
        let user = Sid::current_user().unwrap().to_string_form().unwrap();
        // Control: the same owner-only DACL without a label lets a low-integrity
        // process of the same user read, which is why the label is there.
        let control = unique_endpoint();
        let unlabeled =
            OwnerOnlyDescriptor::from_sddl(&format!("O:{user}D:P(A;;FA;;;{user})")).unwrap();
        let _control_pipe = create_instance(&control, &unlabeled, true).unwrap();
        assert_eq!(
            open_at_low_integrity(&control, GENERIC_READ).map(drop),
            Ok(())
        );

        let endpoint = unique_endpoint();
        let _listener = PipeListener::bind(&endpoint).unwrap();
        assert_eq!(
            open_at_low_integrity(&endpoint, GENERIC_READ).map(drop),
            Err(ERROR_ACCESS_DENIED)
        );
        assert_eq!(
            open_at_low_integrity(&endpoint, GENERIC_READ | GENERIC_WRITE).map(drop),
            Err(ERROR_ACCESS_DENIED)
        );
    }

    /// A descriptor that lets everyone in at any integrity level (an untrusted label): what the listener would have if its own descriptor ever regressed. It names the user as the owner, as the listener does, or an elevated test would get a pipe Administrators own, which its own connector refuses.
    fn regressed() -> String {
        let user = Sid::current_user().unwrap().to_string_form().unwrap();
        format!("O:{user}D:P(A;;FA;;;WD)(A;;FA;;;AN)S:(ML;;NW;;;S-1-16-0)")
    }

    /// Accepts the client that `open` connects, and expects it refused.
    async fn expect_refused<Client>(
        endpoint: &Endpoint,
        listener: &mut PipeListener,
        open: impl FnOnce(&Endpoint) -> Client,
    ) -> String {
        let client = open(endpoint);
        let refused = listener.accept().await;
        drop(client);
        match refused {
            Err(IpcError::PeerRefused { peer }) => peer,
            other => panic!("expected PeerRefused, got {other:?}"),
        }
    }

    // The listener used to take whoever the descriptor let in, so a descriptor that
    // regressed (as the label once did, IPB-1) left nothing else to stop another
    // principal (IPB-9).
    #[tokio::test]
    async fn the_listener_refuses_another_principal_its_descriptor_let_in() {
        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind_behind(&endpoint, &regressed()).unwrap();
        let peer = expect_refused(&endpoint, &mut listener, |endpoint| {
            open_as_anonymous(endpoint, GENERIC_READ | GENERIC_WRITE).unwrap()
        })
        .await;
        assert!(
            peer.contains("S-1-5-7"),
            "names the anonymous logon: {peer}"
        );

        // The refusal concerns that client only: the listener still serves the user.
        let user = Sid::current_user().unwrap();
        let (client, server) = tokio::join!(connect(&endpoint, &user), listener.accept());
        client.unwrap();
        server.unwrap();
    }

    #[tokio::test]
    async fn the_listener_refuses_a_lower_integrity_program_of_the_same_user() {
        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind_behind(&endpoint, &regressed()).unwrap();
        let peer = expect_refused(&endpoint, &mut listener, |endpoint| {
            open_at_low_integrity(endpoint, GENERIC_READ | GENERIC_WRITE).unwrap()
        })
        .await;
        assert!(peer.contains("S-1-16-4096"), "names the low level: {peer}");
    }

    /// Opens the pipe from a thread running as the current user at the current integrity
    /// level but on a restricted token (one whose access is also checked against a list of
    /// restricting SIDs, here only Everyone), the kind a browser's sandbox uses below its
    /// `AppContainer`. Returns the open client end.
    fn open_with_restricted_token(endpoint: &Endpoint) -> Result<OwnedHandle, u32> {
        use windows_sys::Win32::Security::{CreateRestrictedToken, TOKEN_ALL_ACCESS};

        let name = wide(endpoint.as_os_str());
        std::thread::spawn(move || {
            let everyone = Sid::parse("S-1-1-0").unwrap();
            let restricting = SID_AND_ATTRIBUTES {
                Sid: everyone.as_psid(),
                Attributes: 0,
            };
            let mut process_token: HANDLE = ptr::null_mut();
            let mut restricted: HANDLE = ptr::null_mut();
            let mut token: HANDLE = ptr::null_mut();
            // SAFETY: plain Win32 calls on handles opened and closed here; `restricting`
            // and the SID it points at outlive the call that reads them; the
            // impersonation only affects this thread and is reverted.
            unsafe {
                assert_ne!(
                    OpenProcessToken(
                        GetCurrentProcess(),
                        TOKEN_DUPLICATE | TOKEN_QUERY,
                        &raw mut process_token
                    ),
                    0
                );
                assert_ne!(
                    CreateRestrictedToken(
                        process_token,
                        0,
                        0,
                        ptr::null(),
                        0,
                        ptr::null(),
                        1,
                        &raw const restricting,
                        &raw mut restricted,
                    ),
                    0
                );
                CloseHandle(process_token);
                assert_ne!(
                    DuplicateTokenEx(
                        restricted,
                        TOKEN_ALL_ACCESS,
                        ptr::null(),
                        SecurityImpersonation,
                        TokenImpersonation,
                        &raw mut token,
                    ),
                    0
                );
                CloseHandle(restricted);
                assert_ne!(SetThreadToken(ptr::null(), token), 0);
                let handle = CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    ptr::null_mut(),
                );
                let error = GetLastError();
                RevertToSelf();
                CloseHandle(token);
                if handle == INVALID_HANDLE_VALUE {
                    Err(error)
                } else {
                    Ok(OwnedHandle::from_raw_handle(handle as RawHandle))
                }
            }
        })
        .join()
        .unwrap()
    }

    // A restricted token keeps the user's SID and level, so behind a descriptor that let it
    // in, the user and level checks served it (W4I review L4).
    #[tokio::test]
    async fn the_listener_refuses_a_sandboxed_program_of_the_same_user() {
        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind_behind(&endpoint, &regressed()).unwrap();
        let peer = expect_refused(&endpoint, &mut listener, |endpoint| {
            open_with_restricted_token(endpoint).unwrap()
        })
        .await;
        assert!(peer.contains("sandbox"), "{peer}");
    }

    // A client may open the pipe at the anonymous level, which hides who it is from
    // the server; a client that will not say who it is is not served.
    #[tokio::test]
    async fn the_listener_refuses_a_client_that_will_not_say_who_it_is() {
        use windows_sys::Win32::Storage::FileSystem::SECURITY_ANONYMOUS;

        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind(&endpoint).unwrap();
        let peer = expect_refused(&endpoint, &mut listener, |endpoint| {
            ClientOptions::new()
                .security_qos_flags(SECURITY_ANONYMOUS)
                .open(endpoint.as_os_str())
                .unwrap()
        })
        .await;
        assert!(peer.contains("anonymously"), "{peer}");
    }

    // Without a security quality of service, Windows hands the server the client's
    // token only after the client writes; until then the server cannot tell who it is.
    #[tokio::test]
    async fn the_listener_refuses_a_client_that_has_not_said_who_it_is_yet() {
        let endpoint = unique_endpoint();
        let mut listener = PipeListener::bind(&endpoint).unwrap();
        let peer = expect_refused(&endpoint, &mut listener, |endpoint| {
            let name = wide(endpoint.as_os_str());
            // SAFETY: `name` is NUL-terminated; the handle is owned by the result.
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                )
            };
            assert_ne!(handle, INVALID_HANDLE_VALUE);
            // SAFETY: a valid handle we own and nothing else closes.
            unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) }
        })
        .await;
        assert!(peer.contains("identification level"), "{peer}");
    }

    #[test]
    fn a_client_is_served_only_as_the_user_at_the_servers_level_or_above() {
        let user = Sid::current_user().unwrap();
        let other = Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1001").unwrap();
        let level = |text: &str| Sid::parse(text).unwrap();
        let client = |user: &Sid, integrity: &str| {
            Ok(ClientIdentity {
                user: user.clone(),
                integrity: level(integrity),
                sandboxed: false,
            })
        };
        let medium = level("S-1-16-8192");
        assert!(check_client(&user, &medium, client(&user, "S-1-16-8192")).is_ok());
        assert!(check_client(&user, &medium, client(&user, "S-1-16-12288")).is_ok());
        for (who, integrity) in [
            (&other, "S-1-16-8192"),
            (&user, "S-1-16-4096"),
            (&user, "S-1-16-0"),
            // Not a mandatory label at all.
            (&user, "S-1-5-18"),
        ] {
            assert!(
                matches!(
                    check_client(&user, &medium, client(who, integrity)),
                    Err(IpcError::PeerRefused { .. })
                ),
                "{who} at {integrity}"
            );
        }
        let sandboxed = Ok(ClientIdentity {
            user: user.clone(),
            integrity: level("S-1-16-8192"),
            sandboxed: true,
        });
        assert!(matches!(
            check_client(&user, &medium, sandboxed),
            Err(IpcError::PeerRefused { .. })
        ));
        // An elevated server serves only elevated clients.
        let high = level("S-1-16-12288");
        assert!(check_client(&user, &high, client(&user, "S-1-16-8192")).is_err());
        // Failing to read the client for another reason is an error, not a refusal.
        assert!(matches!(
            check_client(&user, &medium, Err(os_error(ERROR_ACCESS_DENIED))),
            Err(IpcError::Io { .. })
        ));
    }

    #[test]
    fn only_a_mandatory_label_has_an_integrity_level() {
        let level = |text: &str| Sid::parse(text).unwrap().integrity_level();
        assert_eq!(level("S-1-16-4096"), Some(4096));
        assert_eq!(level("S-1-16-12288"), Some(12288));
        assert_eq!(level("S-1-5-18"), None);
        assert_eq!(level("S-1-5-21-1-2-3-1001"), None);
        assert_eq!(level("S-1-15-8192"), None);
    }

    #[test]
    fn a_taken_name_is_classified_by_its_owner() {
        let endpoint = unique_endpoint();
        let user = Sid::current_user().unwrap();
        let forged = Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1001").unwrap();
        assert!(matches!(
            classify_taken(&endpoint, &user, Ok(user.clone())),
            IpcError::AlreadyInUse { .. }
        ));
        match classify_taken(&endpoint, &user, Ok(forged.clone())) {
            IpcError::EndpointTaken { owner, .. } => assert_eq!(owner, forged.to_string()),
            other => panic!("expected EndpointTaken, got {other:?}"),
        }
        assert!(matches!(
            classify_taken(&endpoint, &user, Err(os_error(ERROR_ACCESS_DENIED))),
            IpcError::EndpointTaken { .. }
        ));
    }

    #[tokio::test]
    async fn a_name_held_by_a_pipe_that_shuts_us_out_is_reported_as_taken() {
        // Stands in for another user's squatting pipe: its DACL admits only
        // SYSTEM, so the current user cannot open it, as with a real squatter
        // whose DACL leaves us out. (Making a pipe whose owner is truly another
        // account needs a second account or SeRestorePrivilege.)
        let endpoint = unique_endpoint();
        let user = Sid::current_user().unwrap().to_string_form().unwrap();
        let foreign = OwnerOnlyDescriptor::from_sddl(&format!("O:{user}D:P(A;;FA;;;SY)")).unwrap();
        let _squatter = create_instance(&endpoint, &foreign, true).unwrap();
        let err = PipeListener::bind(&endpoint).unwrap_err();
        assert!(matches!(err, IpcError::EndpointTaken { .. }), "{err:?}");
        assert!(err.to_string().contains("nothing was exposed"), "{err}");
    }

    #[tokio::test]
    async fn connector_refuses_a_pipe_owned_by_someone_else() {
        let endpoint = unique_endpoint();
        let _listener = PipeListener::bind(&endpoint).unwrap();
        // Limitation: this does not create a pipe that is really owned by another
        // account; that needs a second account or SeRestorePrivilege, neither of
        // which a test run has. Instead the pipe is real and owned by the current
        // user, and the connector is told to expect a different account. That
        // runs the same code a squatted pipe would (open, read the owner back from
        // Windows, compare, refuse before writing anything); only the roles of
        // "real" and "expected" owner are swapped.
        let forged = Sid::parse("S-1-5-21-1111111111-2222222222-3333333333-1001").unwrap();
        let err = connect(&endpoint, &forged).await.unwrap_err();
        match err {
            IpcError::WrongOwner {
                expected, actual, ..
            } => {
                assert_eq!(expected, forged.to_string());
                assert_eq!(actual, Sid::current_user().unwrap().to_string());
            }
            other => panic!("expected WrongOwner, got {other:?}"),
        }
    }

    #[test]
    fn owner_check_compares_whole_sids() {
        let endpoint = unique_endpoint();
        let user = Sid::current_user().unwrap();
        assert!(verify_owner(&endpoint, &user, &user.clone()).is_ok());
        for forged in ["S-1-5-18", "S-1-1-0", "S-1-5-32-544"] {
            let forged = Sid::parse(forged).unwrap();
            assert!(matches!(
                verify_owner(&endpoint, &user, &forged),
                Err(IpcError::WrongOwner { .. })
            ));
        }
    }

    #[tokio::test]
    async fn a_second_listener_on_the_same_endpoint_is_refused() {
        let endpoint = unique_endpoint();
        let _first = PipeListener::bind(&endpoint).unwrap();
        assert!(matches!(
            PipeListener::bind(&endpoint),
            Err(IpcError::AlreadyInUse { .. })
        ));
    }

    #[tokio::test]
    async fn connecting_with_nobody_listening_reports_not_running() {
        let user = Sid::current_user().unwrap();
        assert!(matches!(
            connect(&unique_endpoint(), &user).await,
            Err(IpcError::NotRunning { .. })
        ));
    }

    #[test]
    fn sddl_grants_only_the_owner_at_its_own_integrity() {
        assert_eq!(
            owner_only_sddl("S-1-5-21-9", "S-1-16-8192"),
            "O:S-1-5-21-9D:P(A;;FA;;;S-1-5-21-9)S:(ML;;NWNR;;;S-1-16-8192)"
        );
    }

    // An elevated app used to label its pipe medium, so any unelevated program of the
    // same user could drive it: a way around UAC.
    #[test]
    fn an_elevated_server_labels_its_pipe_high() {
        let sddl = owner_only_sddl("S-1-5-21-9", "S-1-16-12288");
        assert!(sddl.ends_with("S:(ML;;NWNR;;;S-1-16-12288)"), "{sddl}");
        OwnerOnlyDescriptor::from_sddl(&sddl).expect("Windows accepts the descriptor");
    }

    // The label must follow the integrity it is given, not a fixed level: an elevated app used to label its pipe medium. A test process cannot label above its own level, so a lower one proves the same.
    #[tokio::test]
    async fn a_listener_labels_its_pipe_at_the_given_integrity() {
        let low = Sid::parse("S-1-16-4096").unwrap();
        let listener = PipeListener::bind_labelled(&unique_endpoint(), &low).unwrap();
        let security = read_security(listener.waiting_instance()).unwrap();
        assert_eq!(
            security
                .label
                .iter()
                .map(|ace| &ace.sid)
                .collect::<Vec<_>>(),
            [&low]
        );
    }

    #[test]
    fn the_process_integrity_matches_its_elevation() {
        let integrity = Sid::current_integrity().unwrap().to_string_form().unwrap();
        assert!(integrity.starts_with("S-1-16-"), "{integrity}");
        // Elevated processes run at high integrity (S-1-16-12288) or above.
        let high = !matches!(
            integrity.as_str(),
            "S-1-16-4096" | "S-1-16-8192" | "S-1-16-8448"
        );
        assert_eq!(
            elevation().unwrap() != Elevation::Standard,
            high,
            "{integrity}"
        );
    }

    // TokenIsElevated is also set when user account control is off or for the built-in Administrator, where the browser runs elevated too. Only a split token's full half is a start the person chose.
    #[test]
    fn only_a_chosen_elevation_counts_as_elevated() {
        let cases = [
            (FULL_TOKEN, true, Elevation::Elevated),
            (LIMITED_TOKEN, false, Elevation::Standard),
            (SINGLE_TOKEN, false, Elevation::Standard),
            (SINGLE_TOKEN, true, Elevation::AlwaysAdministrator),
        ];
        for (kind, elevated, expected) in cases {
            assert_eq!(
                classify_elevation(kind, elevated).unwrap(),
                expected,
                "{kind}"
            );
        }
        assert!(classify_elevation(0, false).is_err());
    }
}
