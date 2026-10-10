//! Windows transport: a per-user named pipe with an explicit owner-only security
//! descriptor, and a connector that checks the pipe owner before using it.

use std::ffi::{OsStr, c_void};
use std::fmt;
use std::io;
use std::iter;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr;
use std::time::Duration;

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, PipeMode, ServerOptions};
use tokio::time::{Instant, sleep};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_SUCCESS, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    CopySid, GetLengthSid, GetTokenInformation, IsValidSid, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::{Connection, Endpoint, Inner, IpcError, paths};

/// How long `connect` keeps retrying while every pipe instance is busy. The
/// listener always keeps one instance waiting, so this only covers the short gap
/// while it creates the next one.
const BUSY_WAIT: Duration = Duration::from_secs(2);

pub(crate) fn endpoint() -> Result<Endpoint, IpcError> {
    let sid = Sid::current_user()?
        .to_string_form()
        .map_err(|e| IpcError::io("Could not read your user account", e))?;
    Ok(Endpoint::from_raw(paths::pipe_name(&sid)))
}

/// A security identifier, copied out of whatever buffer Windows returned it in.
///
/// Stored as 32-bit words because a SID is always 8 bytes plus 4 per
/// sub-authority, which keeps the buffer aligned for the APIs that read it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Sid {
    words: Vec<u32>,
}

impl Sid {
    /// The user the current process runs as.
    pub(crate) fn current_user() -> Result<Self, IpcError> {
        current_user_sid().map_err(|e| IpcError::io("Could not read your user account", e))
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn parse(text: &str) -> io::Result<Self> {
        let wide = to_wide(OsStr::new(text));
        let mut psid: PSID = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; on success `psid` receives a
        // LocalAlloc block that the guard frees.
        if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut psid) } == 0 {
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
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &mut out) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _guard = LocalBox(out.cast());
        // SAFETY: `out` is NUL-terminated, so the scan stops inside the string.
        let len = (0..).take_while(|&i| unsafe { *out.add(i) } != 0).count();
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

fn current_user_sid() -> io::Result<Sid> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing; on
    // success `raw` is a new token handle that we own.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a valid handle we own and nothing else closes it.
    let token = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
    let handle = token.as_raw_handle() as HANDLE;

    let mut needed = 0u32;
    // SAFETY: a size query with a null buffer; it fails by design and reports the
    // size in `needed`.
    unsafe { GetTokenInformation(handle, TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 elements give the 8-byte alignment TOKEN_USER (which holds a pointer)
    // needs.
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    let capacity = u32::try_from(buf.len() * 8).unwrap_or(u32::MAX);
    // SAFETY: `buf` is writable for `capacity` bytes.
    if unsafe {
        GetTokenInformation(
            handle,
            TokenUser,
            buf.as_mut_ptr().cast(),
            capacity,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call above filled `buf` with a TOKEN_USER, and `buf` is aligned
    // for it. Its SID pointer points inside `buf`, which outlives the copy.
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    // SAFETY: see above.
    unsafe { Sid::copy_from(user.User.Sid) }
}

/// Frees a block that Windows allocated with LocalAlloc.
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

fn to_wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(iter::once(0)).collect()
}

/// The SDDL for the pipe: owned by `sid`, with a protected DACL (nothing
/// inherited) that grants full file access to `sid` and nothing to anyone else.
pub(crate) fn owner_only_sddl(sid: &str) -> String {
    format!("O:{sid}D:P(A;;FA;;;{sid})")
}

/// A self-relative security descriptor built from [`owner_only_sddl`].
struct OwnerOnlyDescriptor(PSECURITY_DESCRIPTOR);

// SAFETY: the descriptor is a heap block owned by this value and never written
// after creation; Windows only reads it when creating pipe instances.
unsafe impl Send for OwnerOnlyDescriptor {}
// SAFETY: as above, shared access is read-only.
unsafe impl Sync for OwnerOnlyDescriptor {}

impl OwnerOnlyDescriptor {
    fn new(user: &Sid) -> io::Result<Self> {
        let sddl = owner_only_sddl(&user.to_string_form()?);
        let wide = to_wide(OsStr::new(&sddl));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; on success `descriptor` receives a
        // LocalAlloc block that Drop frees.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
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
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
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
        options.create_with_security_attributes_raw(
            endpoint.as_os_str(),
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

/// The server side. Always keeps one instance waiting for the next client.
#[derive(Debug)]
pub(crate) struct PipeListener {
    endpoint: Endpoint,
    descriptor: OwnerOnlyDescriptor,
    next: NamedPipeServer,
}

impl PipeListener {
    pub(crate) fn bind(endpoint: &Endpoint) -> Result<Self, IpcError> {
        let user = Sid::current_user()?;
        let descriptor = OwnerOnlyDescriptor::new(&user).map_err(|e| {
            IpcError::io("Could not prepare the access rules for the connection", e)
        })?;
        let next = create_instance(endpoint, &descriptor, true).map_err(|e| {
            if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) {
                // FILE_FLAG_FIRST_PIPE_INSTANCE fails this way when the name
                // already exists, whoever created it.
                IpcError::AlreadyInUse {
                    endpoint: endpoint.to_string(),
                }
            } else {
                IpcError::io(format!("Could not listen at {endpoint}"), e)
            }
        })?;
        Ok(Self {
            endpoint: endpoint.clone(),
            descriptor,
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
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && started.elapsed() < BUSY_WAIT =>
            {
                // No completion event exists for "an instance became free"
                // (WaitNamedPipe blocks a thread), so wait briefly and retry.
                sleep(Duration::from_millis(10)).await;
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
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

/// Reads the owner of a kernel object from its handle (needs READ_CONTROL, which
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
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _guard = LocalBox(descriptor);
    // SAFETY: `owner` points into `descriptor`, which is alive until the guard
    // drops after this copy.
    unsafe { Sid::copy_from(owner) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        ImpersonateAnonymousToken, RevertToSelf, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_ALL_ACCESS, OPEN_EXISTING};
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThread;

    fn unique_endpoint() -> Endpoint {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        Endpoint::from_raw(format!(
            r"\\.\pipe\wirecatch-test-{}-{n}-{nanos}",
            std::process::id()
        ))
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
    }

    /// Reads the owner and DACL of an object back from Windows.
    fn read_security(handle: RawHandle) -> io::Result<Security> {
        let mut owner: PSID = ptr::null_mut();
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: as in `handle_owner`.
        let status = unsafe {
            GetSecurityInfo(
                handle as HANDLE,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _guard = LocalBox(descriptor);
        // SAFETY: `owner` points into the live descriptor.
        let owner = unsafe { Sid::copy_from(owner) }?;

        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: `descriptor` is valid until the guard drops.
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }

        let aces = if dacl.is_null() {
            None
        } else {
            // SAFETY: zeroed is a valid ACL_SIZE_INFORMATION (plain integers).
            let mut info: ACL_SIZE_INFORMATION = unsafe { mem::zeroed() };
            // SAFETY: `dacl` points into the live descriptor; `info` is the right
            // size for the requested class.
            if unsafe {
                GetAclInformation(
                    dacl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
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
                if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: every ACE starts with an ACE_HEADER.
                let header = unsafe { &*(ace as *const ACE_HEADER) };
                let ace_type = u32::from(header.AceType);
                if ace_type != ACCESS_ALLOWED_ACE_TYPE && ace_type != ACCESS_DENIED_ACE_TYPE {
                    return Err(io::Error::other(format!("unexpected ACE type {ace_type}")));
                }
                // SAFETY: allowed and denied ACEs share the ACCESS_ALLOWED_ACE
                // layout, with the SID starting at `SidStart`.
                let body = unsafe { &*(ace as *const ACCESS_ALLOWED_ACE) };
                // SAFETY: as above.
                let sid = unsafe { Sid::copy_from(ptr::addr_of!(body.SidStart) as PSID) }?;
                aces.push(AceInfo {
                    ace_type,
                    mask: body.Mask,
                    sid,
                });
            }
            Some(aces)
        };
        Ok(Security {
            owner,
            dacl_protected: control & SE_DACL_PROTECTED != 0,
            aces,
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
    }

    #[test]
    fn endpoint_is_named_after_the_current_user() {
        let user = Sid::current_user().unwrap().to_string_form().unwrap();
        assert!(user.starts_with("S-1-5-"), "{user}");
        assert_eq!(
            endpoint().unwrap().as_os_str(),
            OsStr::new(&format!(r"\\.\pipe\wirecatch-app-{user}"))
        );
    }

    #[test]
    fn sid_round_trips_through_its_string_form() {
        let system = Sid::parse("S-1-5-18").unwrap();
        assert_eq!(system.to_string_form().unwrap(), "S-1-5-18");
        assert!(Sid::parse("not a sid").is_err());
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
    /// different security principal than the current user.
    fn open_as_anonymous(endpoint: &Endpoint, access: u32) -> Result<(), u32> {
        let name = to_wide(endpoint.as_os_str());
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
                    0,
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
                // SAFETY: `handle` is a valid handle we own.
                unsafe { CloseHandle(handle) };
                Ok(())
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
        assert_eq!(open_as_anonymous(&control, GENERIC_READ), Ok(()));

        let endpoint = unique_endpoint();
        let _listener = PipeListener::bind(&endpoint).unwrap();
        assert_eq!(
            open_as_anonymous(&endpoint, GENERIC_READ),
            Err(ERROR_ACCESS_DENIED)
        );
        assert_eq!(
            open_as_anonymous(&endpoint, GENERIC_READ | GENERIC_WRITE),
            Err(ERROR_ACCESS_DENIED)
        );
    }

    #[tokio::test]
    async fn connector_refuses_a_pipe_owned_by_someone_else() {
        let endpoint = unique_endpoint();
        let _listener = PipeListener::bind(&endpoint).unwrap();
        // The real pipe is owned by the current user; expecting another account
        // stands in for a pipe squatted by that account.
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
    fn sddl_grants_only_the_owner() {
        assert_eq!(
            owner_only_sddl("S-1-5-21-9"),
            "O:S-1-5-21-9D:P(A;;FA;;;S-1-5-21-9)"
        );
    }
}
