//! Minimal PowerShell adapter for data that is cleanly exposed only through
//! built-in cmdlets (PrintManagement). Scripts are constants fed on stdin;
//! all variable data is passed through `MA_*` environment variables, so
//! nothing is ever interpolated into script text. Every invocation is logged
//! by the caller. No profile is loaded and no network resources are used.

use crate::error::{AppError, AppResult};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(90);

fn powershell_exe() -> std::path::PathBuf {
    // Absolute path avoids PATH hijacking from a USB working directory.
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::path::Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

pub fn run_script(script: &str, env: &[(&str, &str)]) -> AppResult<String> {
    let exe = powershell_exe();
    let mut cmd = Command::new(&exe);
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        debug_assert!(k.starts_with("MA_"));
        cmd.env(k, v);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Avoid a flashing console window; the operation is still logged and
        // shown in the UI, it is not hidden from the technician.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| AppError::AdapterUnavailable(format!("PowerShell could not be started: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| AppError::AdapterUnavailable(format!("PowerShell stdin: {e}")))?;
    }
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_handle = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_handle = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err(AppError::AdapterUnavailable("PowerShell adapter timed out".into()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(AppError::AdapterUnavailable(format!("PowerShell wait failed: {e}"))),
        }
    };
    let out = out_handle.join().unwrap_or_default();
    let err = err_handle.join().unwrap_or_default();
    if !status.success() {
        let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_string();
        return Err(AppError::AdapterUnavailable(format!("PowerShell reported: {first}")));
    }
    Ok(out)
}
