# Testing

```bash
npm run test:all            # tsc + Vitest + cargo test --no-default-features
```

The Rust tests need no WebView, administrator rights or Windows: every engine runs against the fixture platform adapter and real directory trees under `fixtures/`, which are copied to a temp folder per test.

## Fixtures

`python3 scripts/generate_fixtures.py` regenerates:

- `fixtures/source-pc/`: "ACCT-PC-07" (Windows 10 22H2, domain-joined) with the current user `CONTOSO\ann`, the other user `CONTOSO\bob`, an access-denied `svc-backup`, and SYSTEM. It has Chrome (2 profiles), Edge (running), Firefox (a real `places.sqlite`), Outlook signatures, templates, stationery, PST and OST, a wallpaper and theme, Sticky Notes, Start menu, taskbar, Recent and Quick Access entries, the Public Desktop, 4 printers, 2 mapped drives, and 7 apps.
- `fixtures/target-pc/`: "ACCT-PC-21" (Windows 11, Azure AD) with `AzureAD\AnnExample`, plus pre-existing `Documents\Report.docx` and Chrome `Bookmarks` for collision tests.

**Planted secrets.** Browser password, cookie, token and key files, Credential Manager, DPAPI, Vault, Wi-Fi XML, `NTUSER.DAT`, the SAM hive, SSH keys and `.pfx` files contain `FIXTURE-SECRET-DO-NOT-COPY`. Tests assert that this marker never appears anywhere in a bundle (payload, manifest, reports or logs) or on a restore target.

## Rust test map

| Requirement | Test |
|---|---|
| Manifest schema, sample validity, no secret fields | `bundle::tests::*`, `capture_tests::malformed_manifests_are_rejected` |
| Path sanitation and traversal | `security::safe_path::tests::*`, `capture::hashing::tests::hash_list_roundtrip_and_validation` |
| Exclusion rules / sensitive categories | `security::exclusions::tests::*`, `capture_tests::sensitive_data_is_excluded_even_when_everything_is_selected`, `discovery::browsers::tests::allow_lists_never_contain_secrets` |
| Collision policies | `restore::files::tests::collision_policies`, `restore_tests::rename_and_replace_policies` |
| Encryption metadata and round trip | `security::encryption::tests::*`, `capture_tests::encrypted_capture_hides_content_and_metadata_holds_no_secret`, `restore_tests::encrypted_bundle_requires_correct_passphrase` |
| Size formatting and ETA | `util::tests::*` (Rust), `src/lib/format.test.ts` (TS) |
| Report redaction | `security::redaction::tests::*`, `reporting::tests::summary_redacts_identities` |
| Integration with fixture profile trees | `capture_tests::*`, `restore_tests::*`, `session_tests::*` |
| Interrupted capture → resume | `capture_tests::interrupted_capture_resumes_and_verifies` |
| Restore dry run | `restore_tests::dry_run_plan_writes_nothing_and_reports_conflicts` |
| Hash verification failure | `capture_tests::hash_verification_detects_tampering`, `restore_tests::tampered_bundle_is_not_restorable`, `restore_tests::file_corrupted_after_validation_is_never_placed` |
| Insufficient disk space | `capture_tests::insufficient_disk_space_blocks_before_writing` |
| Locked-file skip | `capture_tests::locked_files_are_retried_then_skipped_with_warning` |
| Confirmation gating | `restore_tests::unconfirmed_categories_write_nothing` |
| Portable paths | `app_paths::tests::*`, `session_tests::authorization_is_required_and_persisted_portably` |
| PowerShell printer parsing (no injection) | `windows::printers_parse::tests::*` |

## UI tests (Vitest + Testing Library, jsdom)

`tests/ui.test.tsx` drives the real React app against the mock backend: the authorization gate, the scan → select → encryption validation → preflight → capture → truthful completion flow, privacy-sensitive gating, and the restore flow (verification → replace confirmation → dry run with exact system changes → per-category confirmation). The selection rules and formatting have unit tests in `src/lib/*.test.ts`.

## Manual verification on Windows

1. `npm run tauri dev` as a standard user. The scan should list your profile. Other profiles should show as Admin required or Access denied.
2. Capture to a USB stick (exFAT and FAT32), with a browser open (preflight should warn), then cancel mid-way and resume.
3. Run `sha256sum -c` (Git Bash) or `Get-FileHash` against `hashes/*.sha256`.
4. Restore on a second PC or VM as a different account name, and check that conflicts are skipped by default, the bookmarks HTML exports, drive mapping prompts for credentials, and the wallpaper registry value shown in the plan matches what gets written.

## Screenshots

`npm run build && npm run screenshots` regenerates `docs/screenshots/` using a local Chromium (`CHROMIUM_PATH`).
