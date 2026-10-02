# Building and releasing the portable executable

## Requirements (Windows 10/11 x64 build machine)

- Node.js 20 LTS, Rust stable (MSVC toolchain, `x86_64-pc-windows-msvc`), Visual Studio Build Tools with the C++ workload
- WebView2 runtime (preinstalled on Windows 11)
- Optional: an Authenticode code-signing certificate and `signtool.exe` (Windows SDK)

## Build

```powershell
.\scripts\build-portable.ps1
```

The script:

1. runs `npm ci` and the test suite (`npm run test:all`; skip it with `-SkipTests`),
2. runs `npm run tauri build -- --no-bundle`, which produces `src-tauri\target\release\MigrationAssistant.exe` with no installer,
3. creates `dist-portable\MigrationAssistant\` containing the executable, `README.txt` and `SHA256SUMS.txt`, plus the fixed WebView2 runtime if `-WebView2FixedRuntime <dir>` is given.

The executable is self-contained: frontend assets are embedded, and it needs no Node.js, Python, installer or service. The release profile uses LTO, `opt-level = "s"` and stripped symbols.

## WebView2 options

| Situation | Option |
|---|---|
| Windows 11 or updated Windows 10 | Nothing to do: the Evergreen runtime is present. |
| Offline or locked-down Windows 10 without WebView2 | Download the **Fixed Version** runtime (x64 CAB) from Microsoft, extract it, and pass `-WebView2FixedRuntime`. The script copies it to `MigrationAssistant\WebView2\`. Set `"webviewInstallMode": {"type": "fixedRuntime", "path": "./WebView2/"}` in `src-tauri/tauri.conf.json` before building. |

The default config uses `"skip"` because a portable tool must not install anything on target machines.

## Code signing ("signed-ready")

```powershell
.\scripts\sign-windows.ps1 -Path dist-portable\MigrationAssistant\MigrationAssistant.exe -Thumbprint <SHA1 thumbprint>
```

This signs with SHA-256 and an RFC 3161 timestamp (`/tr http://timestamp.digicert.com` by default; override with `-TimestampUrl`). Signing is a build-machine step only: the application itself never makes network requests. Regenerate `SHA256SUMS.txt` after signing (the script does this for you).

## Release checklist

- [ ] `npm run test:all` is green on Windows and Linux CI
- [ ] Manual verification steps in [TESTING.md](TESTING.md)
- [ ] Version bumped in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
- [ ] Executable signed and checksums published
- [ ] README limitations reviewed for the release

## Optional installer

The bundle is disabled (`"bundle.active": false`). If an organization wants an installer, set `"active": true` (the NSIS target is preconfigured) and run `npm run tauri build`. The portable executable remains the primary deliverable.
