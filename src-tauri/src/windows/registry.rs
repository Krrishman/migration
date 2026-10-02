//! Read-only registry helpers. Migration Assistant reads the registry for
//! inventory only; the single registry write (wallpaper) goes through
//! SystemParametersInfoW after explicit confirmation.

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winreg::RegKey;

#[derive(Debug, Clone, Copy)]
pub enum Hive {
    Hklm64,
    Hklm32,
    Hkcu,
}

fn open(hive: Hive, path: &str) -> Option<RegKey> {
    let (root, flags) = match hive {
        Hive::Hklm64 => (RegKey::predef(HKEY_LOCAL_MACHINE), KEY_READ | KEY_WOW64_64KEY),
        Hive::Hklm32 => (RegKey::predef(HKEY_LOCAL_MACHINE), KEY_READ | KEY_WOW64_32KEY),
        Hive::Hkcu => (RegKey::predef(HKEY_CURRENT_USER), KEY_READ),
    };
    root.open_subkey_with_flags(path, flags).ok()
}

pub fn string_value(hive: Hive, path: &str, name: &str) -> Option<String> {
    open(hive, path)?.get_value::<String, _>(name).ok()
}

pub fn dword_value(hive: Hive, path: &str, name: &str) -> Option<u32> {
    open(hive, path)?.get_value::<u32, _>(name).ok()
}

pub fn subkeys(hive: Hive, path: &str) -> Vec<String> {
    open(hive, path).map(|k| k.enum_keys().filter_map(Result::ok).collect()).unwrap_or_default()
}

pub fn hklm_string(path: &str, name: &str) -> Option<String> {
    string_value(Hive::Hklm64, path, name)
}
pub fn hklm_dword(path: &str, name: &str) -> Option<u32> {
    dword_value(Hive::Hklm64, path, name)
}
pub fn hklm_subkeys(path: &str) -> Vec<String> {
    subkeys(Hive::Hklm64, path)
}
pub fn hklm_has_subkeys(path: &str) -> bool {
    !hklm_subkeys(path).is_empty()
}
pub fn hkcu_string(path: &str, name: &str) -> Option<String> {
    string_value(Hive::Hkcu, path, name)
}
pub fn hkcu_subkeys(path: &str) -> Vec<String> {
    subkeys(Hive::Hkcu, path)
}
