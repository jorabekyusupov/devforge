use crate::{Endpoint, Error, PeerIdentity, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::{Pipes::*, SystemServices::ACCESS_ALLOWED_ACE_TYPE, Threading::*},
};

struct LocalMemory(*mut core::ffi::c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn sid_string(sid: PSID) -> Result<String> {
    let mut text = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(Error::UnauthorizedPeer);
    }
    let memory = LocalMemory(text.cast());
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
        if length > 256 {
            return Err(Error::UnauthorizedPeer);
        }
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
        .map_err(|_| Error::UnauthorizedPeer)?;
    drop(memory);
    Ok(result)
}
fn user_for_process(process: HANDLE) -> Result<String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(Error::UnauthorizedPeer);
    }
    let token = Handle(token);
    let mut length = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65_536 {
        return Err(Error::UnauthorizedPeer);
    }
    // TOKEN_USER requires pointer alignment, unlike a Vec<u8> allocation.
    let mut bytes = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            bytes.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(Error::UnauthorizedPeer);
    }
    let user = unsafe { &*bytes.as_ptr().cast::<TOKEN_USER>() };
    sid_string(user.User.Sid)
}
pub(crate) fn current_user() -> Result<String> {
    user_for_process(unsafe { GetCurrentProcess() })
}
pub(crate) fn current_user_digest() -> Result<String> {
    Ok(Sha256::digest(current_user()?.as_bytes())
        .iter()
        .take(12)
        .map(|b| format!("{b:02x}"))
        .collect())
}
fn descriptor(directory: bool) -> Result<LocalMemory> {
    let user = current_user()?;
    let flags = if directory { "OICI" } else { "" };
    let sddl = wide(std::ffi::OsStr::new(&format!(
        "O:{user}D:P(A;{flags};FA;;;{user})"
    )));
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(LocalMemory(descriptor))
}
fn security(path: &Path) -> Result<(LocalMemory, PSID, *mut ACL)> {
    let name = wide(path.as_os_str());
    let mut sd = std::ptr::null_mut();
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let result = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if result != 0 {
        return Err(Error::UnsafeEndpoint);
    }
    Ok((LocalMemory(sd), owner, dacl))
}
pub(crate) fn validate_root(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::UnsafeEndpoint);
    }
    let (_sd, owner, _dacl) = security(path)?;
    if owner.is_null() || sid_string(owner)? != current_user()? {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
pub(crate) fn validate_private_dir(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::UnsafeEndpoint);
    }
    let (sd, owner, dacl) = security(path)?;
    if owner.is_null() || sid_string(owner)? != current_user()? || dacl.is_null() {
        return Err(Error::UnsafeEndpoint);
    }
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(sd.0, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
        || unsafe { (*dacl).AceCount } != 1
    {
        return Err(Error::UnsafeEndpoint);
    }
    let mut ace = std::ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 || ace.is_null() {
        return Err(Error::UnsafeEndpoint);
    }
    let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    if allowed.Header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
        || allowed.Header.AceFlags != (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8
        || allowed.Mask != FILE_ALL_ACCESS
        || sid_string((&allowed.SidStart as *const u32).cast_mut().cast())? != current_user()?
    {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}
pub(crate) fn ensure_private_dir(path: &Path) -> Result<()> {
    let sd = descriptor(true)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let path_wide = wide(path.as_os_str());
    if unsafe { CreateDirectoryW(path_wide.as_ptr(), &attributes) } == 0 {
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e.into());
        }
    }
    validate_private_dir(path)
}
pub(crate) fn secure_lock_options(options: &mut OpenOptions) {
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}
pub(crate) fn validate_lock(file: &File) -> Result<()> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0
        || info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(Error::UnsafeEndpoint);
    }
    Ok(())
}

pub(crate) enum Stream {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}
impl Stream {
    fn handle(&self) -> HANDLE {
        match self {
            Self::Server(s) => s.as_raw_handle() as HANDLE,
            Self::Client(s) => s.as_raw_handle() as HANDLE,
        }
    }
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(s) => Pin::new(s).poll_read(cx, buf),
            Self::Client(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Server(s) => Pin::new(s).poll_write(cx, buf),
            Self::Client(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(s) => Pin::new(s).poll_flush(cx),
            Self::Client(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(s) => Pin::new(s).poll_shutdown(cx),
            Self::Client(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
pub(crate) struct Listener {
    endpoint: Endpoint,
    next: NamedPipeServer,
}
fn pipe(endpoint: &Endpoint, first: bool) -> Result<NamedPipeServer> {
    let sd = descriptor(false)?;
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        // accept() creates the next listener before returning the connected
        // stream. At capacity that transiently needs: active exchanges + the
        // excess peer (which the caller rejects) + the replacement listener.
        // Without this extra instance, saturation would terminate the owner.
        .max_instances(crate::MAX_CONNECTIONS + 2);
    Ok(unsafe {
        options.create_with_security_attributes_raw(
            endpoint.address.as_os_str(),
            (&mut attrs as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }?)
}
impl Listener {
    pub(crate) fn bind(endpoint: &Endpoint) -> Result<Self> {
        Ok(Self {
            endpoint: endpoint.clone(),
            next: pipe(endpoint, true)?,
        })
    }
    pub(crate) async fn accept(&mut self) -> Result<Stream> {
        self.next.connect().await?;
        let next = pipe(&self.endpoint, false)?;
        Ok(Stream::Server(std::mem::replace(&mut self.next, next)))
    }
}
pub(crate) async fn connect(endpoint: &Endpoint) -> Result<Stream> {
    loop {
        match ClientOptions::new()
            .security_qos_flags(SECURITY_IDENTIFICATION)
            .open(&endpoint.address)
        {
            Ok(client) => return Ok(Stream::Client(client)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await
            }
            Err(e) => return Err(e.into()),
        }
    }
}
pub(crate) fn peer(stream: &Stream, server: bool) -> Result<PeerIdentity> {
    let mut pid = 0;
    let result = unsafe {
        if server {
            GetNamedPipeServerProcessId(stream.handle(), &mut pid)
        } else {
            GetNamedPipeClientProcessId(stream.handle(), &mut pid)
        }
    };
    if result == 0 || pid == 0 {
        return Err(Error::UnauthorizedPeer);
    }
    let process = Handle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if process.0.is_null() {
        return Err(Error::UnauthorizedPeer);
    }
    let user = user_for_process(process.0)?;
    if user != current_user()? {
        return Err(Error::UnauthorizedPeer);
    }
    Ok(PeerIdentity {
        user,
        process_id: Some(pid),
    })
}
pub(crate) fn cleanup(_endpoint: &Endpoint) -> Result<()> {
    Ok(())
}
