//! Thin, safe wrappers over the Win32 APIs used by the Windows adapter.
//! Every function degrades to `None`/`false`/an error rather than panicking.

#![cfg(windows)]

use crate::error::{AppError, AppResult};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// True if the current process token is elevated (UAC "Run as administrator").
pub fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Resolve a string SID to `DOMAIN\name` using LookupAccountSidW.
pub fn lookup_account(sid: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows_sys::Win32::Security::LookupAccountSidW;
    unsafe {
        let mut psid = null_mut();
        let w = wide(sid);
        if ConvertStringSidToSidW(w.as_ptr(), &mut psid) == 0 {
            return None;
        }
        let mut name = [0u16; 256];
        let mut domain = [0u16; 256];
        let mut name_len = name.len() as u32;
        let mut domain_len = domain.len() as u32;
        let mut use_ = 0;
        let ok = LookupAccountSidW(null(), psid, name.as_mut_ptr(), &mut name_len, domain.as_mut_ptr(), &mut domain_len, &mut use_);
        LocalFree(psid as _);
        if ok == 0 {
            return None;
        }
        let (n, d) = (from_wide(&name), from_wide(&domain));
        Some(if d.is_empty() { n } else { format!("{d}\\{n}") })
    }
}

/// Workgroup/domain join state via NetGetJoinInformation.
pub fn join_information() -> (crate::models::JoinState, Option<String>) {
    use crate::models::JoinState;
    use windows_sys::Win32::NetworkManagement::NetManagement::{NetApiBufferFree, NetGetJoinInformation, NetSetupDomainName, NetSetupWorkgroupName};
    unsafe {
        let mut buf = null_mut();
        let mut status = 0;
        if NetGetJoinInformation(null(), &mut buf, &mut status) != 0 {
            return (JoinState::Unknown, None);
        }
        let name = if buf.is_null() {
            None
        } else {
            let mut len = 0;
            while *buf.add(len) != 0 {
                len += 1;
            }
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(buf, len)))
        };
        if !buf.is_null() {
            NetApiBufferFree(buf as _);
        }
        match status {
            s if s == NetSetupDomainName => (JoinState::Domain, name),
            s if s == NetSetupWorkgroupName => (JoinState::Workgroup, name),
            _ => (JoinState::Unknown, name),
        }
    }
}

/// Drive letters currently connected to network shares (A: .. Z:).
pub fn remote_drive_connections() -> Vec<(String, String)> {
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_REMOTE: u32 = 4;
    let mut out = Vec::new();
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = format!("{}:", (b'A' + i as u8) as char);
            let root = wide(&format!("{letter}\\"));
            if GetDriveTypeW(root.as_ptr()) != DRIVE_REMOTE {
                continue;
            }
            let local = wide(&letter);
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            if WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) == 0 {
                out.push((letter, from_wide(&buf)));
            }
        }
    }
    out
}

/// Map a drive letter through the normal Windows network provider. With
/// CONNECT_INTERACTIVE | CONNECT_PROMPT Windows shows its own credential
/// prompt when needed; Migration Assistant never sees or stores credentials.
pub fn map_drive(letter: &str, unc: &str, persistent: bool) -> AppResult<()> {
    use windows_sys::Win32::NetworkManagement::WNet::{
        WNetAddConnection3W, CONNECT_INTERACTIVE, CONNECT_PROMPT, CONNECT_UPDATE_PROFILE, NETRESOURCEW, RESOURCETYPE_DISK,
    };
    let mut local = wide(letter);
    let mut remote = wide(unc);
    let mut nr: NETRESOURCEW = unsafe { std::mem::zeroed() };
    nr.dwType = RESOURCETYPE_DISK;
    nr.lpLocalName = local.as_mut_ptr();
    nr.lpRemoteName = remote.as_mut_ptr();
    let mut flags = CONNECT_INTERACTIVE | CONNECT_PROMPT;
    if persistent {
        flags |= CONNECT_UPDATE_PROFILE;
    }
    let rc = unsafe { WNetAddConnection3W(null_mut(), &nr, null(), null(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(format!("Windows could not map {letter} to {unc} (error {rc}).")))
    }
}

/// Set the current user's desktop wallpaper via SystemParametersInfoW.
pub fn set_wallpaper(path: &std::path::Path) -> AppResult<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER};
    let mut w = wide(&path.display().to_string());
    let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, w.as_mut_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    if ok != 0 { Ok(()) } else { Err(AppError::InvalidRequest("Windows rejected the wallpaper change".into())) }
}

/// Relaunch the current executable with the "runas" verb (standard UAC prompt).
pub fn restart_elevated(args: &[String]) -> AppResult<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let exe = std::env::current_exe().map_err(|e| AppError::io("current_exe", e))?;
    let verb = wide("runas");
    let file = wide(&exe.display().to_string());
    let params = wide(
        &args
            .iter()
            .map(|a| format!("\"{}\"", a.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let h = unsafe { ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), null(), SW_SHOWNORMAL) };
    // Per the API contract, values > 32 indicate success.
    if h as isize > 32 {
        Ok(())
    } else {
        Err(AppError::InvalidRequest("Elevation was canceled or failed. Continuing without administrator rights.".into()))
    }
}
