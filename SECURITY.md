# Security

Migration Assistant is an **authorized IT migration tool**. It is deliberately not a monitoring tool, collector, credential dumper or remote-access tool. This document describes the threat model and the safeguards built into the code.

## Product boundaries (enforced in code)

| Never | How it is enforced |
|---|---|
| Network access, telemetry, remote control | No HTTP or socket code. The Tauri capability set is `core:default` + `dialog:allow-open` only (no `http`, `shell`, `fs` or `opener` plugins). The CSP allows only `self` and Tauri IPC. Fonts and icons are bundled. |
| Credential or secret extraction | A non-overridable sensitive exclusion list (`security/exclusions.rs`) and allow-list-only browser capture (`discovery/browsers.rs`). Tests plant secret marker files in the fixtures and assert the marker never appears anywhere in a bundle or on a restore target. |
| Bypassing ACLs or Windows/browser protection | Files are opened normally with the user's token. Access-denied items are marked and skipped. Elevation only happens through the standard UAC prompt (`ShellExecuteW "runas"`). There is no DPAPI, no `SeBackupPrivilege`, no raw disk or VSS access, and no decryption of protected data. |
| Silent overwrite or deletion | Restore defaults to Skip existing. Replace requires an extra per-category confirmation and keeps the original as `.bak`. Source data is only opened for reading. Bundle deletion is limited to incomplete or unverified bundles inside a `migrations` folder with the bundle layout, and requires typing the bundle ID. |
| Copying programs, licenses or system files | System roots (and folders that contain them) are refused. Installed apps are inventory only. |
| Force-closing user applications | Not implemented. Locked files are retried, skipped and reported. |

### Excluded categories (always)

- **Browsers:** `Login Data*`, `Cookies*`, `Network/` (cookies), `Web Data*` (autofill and cards), `Account Web Data`, `Local State` (os_crypt key), `Secure Preferences`, `Trust Tokens`, `Sync Data`, session stores, `Local Storage`, `IndexedDB`, service workers, and passkeys. For Firefox: `logins.json`, `key3.db`, `key4.db`, `cert8/9.db`, `signons.sqlite`, `cookies.sqlite`, `formhistory.sqlite`, `signedInUser.json`, `pkcs11.txt` and session files.
- **Windows:** `AppData\*\Microsoft\Credentials`, `Protect` (DPAPI master keys), `Vault`, `Crypto`, `SystemCertificates\My`, `IdentityCache`, `TokenBroker`, AAD broker plug-in state, `ProgramData\Microsoft\Wlansvc` (Wi-Fi), `ProgramData\Microsoft\Crypto`, `System32\config`, and the `NTUSER.DAT` / `UsrClass.dat` hives.
- **Keys and credential files anywhere:** `id_rsa`, `id_ed25519`, `id_ecdsa`, `id_dsa`, `.ssh/`, `.gnupg/`, `*.pfx`, `*.p12`, `*.ppk`, `*.kdbx`, `*.rdg`.
- **OS noise:** `pagefile.sys`, `hiberfil.sys`, `swapfile.sys`, recycle bins, `System Volume Information`, temp folders and files, and caches (unless caches are explicitly enabled; even then, sensitive files stay excluded).

Every bundle's manifest lists these rules under `exclusions`.

## Path safety

`security/safe_path.rs`:

- `safe_relative` rejects empty, absolute, drive-letter, UNC, `..`, alternate-data-stream (`:`) and control-character paths. It is used for **every** path read from a manifest or hash list, so a hostile bundle cannot write outside the restore target or read outside the bundle (`join_within`).
- `is_within` is component-aware and case-insensitive, and strips the `\\?\` and `\\?\UNC\` prefixes.
- `sanitize_component` and `sanitize_dir_name` handle reserved device names (`CON`, `NUL`, `COM1` …), invalid characters, trailing dots and spaces, and length.
- The walker uses `symlink_metadata` and **never follows symlinks or junctions** (name-surrogate reparse points). Cloud placeholders are detected from their attributes and are never opened, so they are never downloaded.
- Preflight refuses destinations inside a system root or inside a folder being captured, to prevent recursion.

## Encryption at rest (optional)

- **KDF:** Argon2id v19, 64 MiB, 3 iterations, 1 lane, and a 16-byte random salt per bundle. Parameters read from a manifest are bounded, so a hostile manifest cannot request absurd amounts of memory.
- **Per-file key:** HKDF-SHA256(bundle key, 32-byte random salt per file).
- **Cipher:** AES-256-GCM in the STREAM construction (BE32 counter plus last-block flag), with a 7-byte random nonce prefix per file and 1 MiB chunks. Truncation, reordering and tampering are detected. On restore, plaintext is written to a temp file and moved into place only after authentication **and** a SHA-256 match.
- **Passphrase:** at least 12 characters with a confirmation field. It is held in memory only. The request field is `skip_serializing`, so it never reaches `capture-request.json`, logs or the manifest. A tested key-check value detects a wrong passphrase. Key material is zeroized on drop.
- **Not encrypted:** file names, sizes and inventory metadata in the manifest. This keeps bundles inspectable and verifiable without the passphrase; the trade-off is documented in the UI.
- If the passphrase is lost, the data cannot be recovered. The UI states this next to the passphrase fields.

## PowerShell usage

Only `Get-Printer`, `Get-PrinterPort` and `Get-PrinterDriver` are used, plus `Add-Printer` / `Add-PrinterPort` after explicit confirmation on restore. Scripts are compile-time constants fed on stdin to `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -NonInteractive` (an absolute path, so a malicious `powershell.exe` on the USB drive cannot be picked up from PATH). All data is passed through `MA_*` environment variables, so there is no string interpolation into script text. There is a 90-second timeout, and failures degrade to a warning.

## Elevation

The app runs `asInvoker`. "Restart elevated" relaunches the same executable through UAC with the reason modules as arguments, and the current instance exits. Items that need administrator rights are labelled **Admin required** in discovery and are blocked with a clear reason in restore plans when the app is not elevated.

## Logs and reports

- Logs (`logs/*.log.jsonl`) contain paths, counts and error kinds only. Messages are built by the app; secret values never enter the logging path.
- Technician reports stay inside the bundle (local by default). The **summary report** passes all text through `security/redaction.rs`, which replaces account names, profile paths, the computer name, the domain, UNC server names and e-mail addresses.
- HTML reports escape every value, contain no scripts, and set a restrictive CSP meta tag.

## Supply chain

Dependencies are mainstream crates and npm packages, with versions pinned by the lockfiles. Release builds use LTO and strip symbols. The executable is signing-ready (`scripts/sign-windows.ps1`, Authenticode SHA-256 with an RFC 3161 timestamp).

## Reporting a vulnerability

Please report security issues privately to the maintainers, not through public issues. Include the version, steps to reproduce and impact. We aim to acknowledge reports within 5 business days.
