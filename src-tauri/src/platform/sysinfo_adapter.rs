//! Cross-platform helpers built on `sysinfo`: CPU/RAM, disks, network
//! adapters, processes and free space. Used by the Windows adapter and by the
//! fixture adapter for real disk-space numbers.

use crate::models::*;
use crate::security::safe_path::{is_within, normalize_for_compare};
use std::path::Path;
use sysinfo::{Disks, Networks, ProcessesToUpdate, System};

pub fn cpu_and_memory() -> (String, usize, u64) {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_else(|| "Unknown CPU".into());
    (brand, sys.cpus().len(), sys.total_memory())
}

pub fn drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|d| DriveInfo {
            mount_point: d.mount_point().display().to_string(),
            label: d.name().to_string_lossy().to_string(),
            file_system: d.file_system().to_string_lossy().to_string(),
            kind: if d.is_removable() { DriveKind::Removable } else { DriveKind::Fixed },
            total_bytes: d.total_space(),
            free_bytes: d.available_space(),
        })
        .collect()
}

pub fn network_adapters() -> Vec<NetworkAdapterInfo> {
    let nets = Networks::new_with_refreshed_list();
    let mut v: Vec<NetworkAdapterInfo> = nets
        .iter()
        .map(|(name, data)| NetworkAdapterInfo { name: name.clone(), mac_address: data.mac_address().to_string() })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub fn processes() -> Vec<ProcessInfo> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let mut v: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessInfo { name: p.name().to_string_lossy().to_string(), pid: pid.as_u32() })
        .collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Free/total space of the volume containing `path` (longest mount-point match).
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| is_within(d.mount_point(), &target))
        .max_by_key(|d| normalize_for_compare(d.mount_point()).len())
        .map(|d| DiskSpace { path: path.display().to_string(), total_bytes: d.total_space(), free_bytes: d.available_space() })
}

pub fn file_system_of(path: &Path) -> Option<String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| is_within(d.mount_point(), &target))
        .max_by_key(|d| normalize_for_compare(d.mount_point()).len())
        .map(|d| d.file_system().to_string_lossy().to_string())
}

pub fn host_name() -> String {
    std::env::var("COMPUTERNAME").ok().or_else(System::host_name).unwrap_or_else(|| "UNKNOWN-PC".into())
}
