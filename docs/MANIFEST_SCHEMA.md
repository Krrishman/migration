# Manifest schema (version 1.0)

`manifest.json` sits at the root of every bundle. `manifest.sha256` contains `"<sha256>  manifest.json"` and is rewritten after every change, including appended restore history. A complete real example is [`SAMPLE_MANIFEST.json`](../SAMPLE_MANIFEST.json). The Rust definition is in `src-tauri/src/models/manifest.rs` and is authoritative.

**Compatibility rule:** readers reject a different **major** `schema_version`. Minor versions may only add optional fields. Unknown fields are ignored for forward compatibility, and semantic validation (`bundle::validate_manifest`) runs after parsing.

**No secrets:** the schema has no field for passwords, tokens, cookies, keys or credentials. A test walks `SAMPLE_MANIFEST.json` and fails if any key is named `password`, `passphrase`, `token`, `cookie`, `secret`, `private_key` or `key`.

## Top level

| Field | Type | Notes |
|---|---|---|
| `schema_version` | string | `"1.0"` |
| `bundle_id` | UUID string | Must parse as a UUID |
| `app_version` | string | Migration Assistant version that created the bundle |
| `created_at` / `completed_at` | RFC 3339 / null | UTC |
| `status` | enum | `in_progress`, `canceled`, `completed_unverified`, `verified`, `verified_with_warnings`. Only the last two can be restored. |
| `source_machine` | object | `computer_name`, `os_name`, `os_version`, `os_build`, `architecture`, `time_zone`, `join_state` (`workgroup`/`domain`/`azure_ad`/`unknown`), `join_name`, `device_id` (always null: not collected) |
| `destination_root` | string | Destination chosen at capture time (informational) |
| `elevated` | bool | Whether capture ran elevated |
| `encryption` | object | See below |
| `users` | array | See below |
| `selected_modules` | string[] | Categories present |
| `items` | array | See below |
| `printers`, `mapped_drives`, `applications`, `browser_profiles` | arrays | Inventories (selected entries only) |
| `restore_compatibility_notes` | string[] | |
| `log_summary` | object | `log_file`, `info_count`, `warning_count`, `error_count`, `errors[]` |
| `capacity` | object | Source and destination free/total bytes at capture time, `estimated_bundle_bytes` |
| `integrity` | object | See below |
| `exclusions` | `{pattern, reason}[]` | Every exclusion rule applied |
| `restore_history` | array | Appended after each restore |

## `encryption`

| Field | Notes |
|---|---|
| `enabled` | bool |
| `algorithm` | `"aes-256-gcm-stream-be32"` |
| `kdf` | `{algorithm: "argon2id-v19", salt (hex, 16 bytes), memory_kib, iterations, parallelism}` |
| `key_check` | Hex of nonce‖AES-GCM(constant). Used only to verify a passphrase; it does not reveal the key. |
| `chunk_size` | 1048576 |

## `users[]`

`sid`, `account_name`, `display_name`, `profile_path`, `last_use`, `profile_size` (approximate), `bundle_dir` (e.g. `users/ann`), `selected_modules` (item IDs).

## `items[]`

| Field | Notes |
|---|---|
| `id` | Deterministic `module:owner-sid:key` |
| `category` | `users_files`, `browsers`, `outlook_email`, `personalization`, `printers`, `network_drives`, `desktop_shortcuts`, `application_settings`, `installed_applications`, `system_inventory` |
| `display_name`, `source_path` | Human-readable |
| `owner` | `{sid, account_name}` or null (machine) |
| `profile_relative` | Path of the source root relative to the owner's profile (used for mapping on restore) |
| `bundle_path` | Bundle-relative folder (or inventory file). Must pass `safe_relative`. |
| `restore_kind` | Tagged union (`type`): `known_folder{folder}`, `custom_folder`, `one_drive_local`, `public_desktop`, `start_menu_shortcuts`, `taskbar_pins`, `quick_access`, `recent_items`, `browser_profile{browser, profile_dir}`, `outlook_signatures`, `outlook_templates`, `outlook_stationery`, `pst_file`, `office_templates`, `wallpaper`, `themes`, `sticky_notes`, `mapped_drives`, `printers`, `inventory{name}` |
| `support` | `supported`, `partial`, `inventory_only`, `unsupported` |
| `estimated_size`, `captured_bytes`, `captured_files`, `skipped_files` | `captured_bytes` counts plaintext bytes |
| `hash_status` | `not_hashed`, `hashed`, `verified` (re-read after write), `mismatch` |
| `hash_list` | `hashes/<task-key>.sha256` (sha256sum format, stored bytes). Encrypted bundles also have `.plain.sha256`. |
| `hash_list_sha256` | Digest of the list file |
| `capture_status` | `pending`, `captured`, `captured_with_warnings`, `skipped`, `failed`, `canceled` |
| `warnings[]` | `{code, severity, message, path?}` |
| `restore_notes[]` | Shown in plans and reports |

## `integrity`

`strategy` (verbatim text of the hashing strategy), `hash_algorithm` (`SHA-256`), `verified`, `verified_at`, `total_files`, `total_bytes`, `mismatches`, and `bundle_root_hash`, which is SHA-256 over each item's `hash_list_sha256` followed by `\n`, in item order.

## `restore_history[]`

`restore_id`, `started_at`, `finished_at`, `target_computer`, `app_version`, `user_mappings` (`[source_sid, target_sid]` pairs), `restored_items`, `files_written`, `files_skipped`, `failures`, `outcome`, `report_path`.

## Bundle layout

```text
<destination_root>/migrations/<COMPUTER>-<YYYY-MM-DD_HHmmss>-<8-hex-id>/
  manifest.json  manifest.sha256  report.html  report.json  summary-report.html  [RESTORE-INSTRUCTIONS.txt]
  logs/       capture.log.jsonl  checkpoint.sqlite  capture-request.json  restore-*.log.jsonl  restore-report-*.html
  inventory/  machine.json  profiles.json  applications.json  reinstall-checklist.txt  outlook.json
  system/     printers.json  network-drives.json  public-desktop/
  users/<account>/
              files/<Known folder>/…   files/Custom/<name>-<id>/…   files/OneDrive/…
              browsers/<chrome|edge|firefox|chromium>/<profile>-<id>/…
              outlook/{signatures,templates,stationery,pst/<id>}/…
              personalization/{wallpaper,themes,sticky-notes}/…
              shortcuts/{start-menu,taskbar,quick-access,recent}/…
              app-settings/office-<id>/…
  hashes/     <module>-<key>.sha256 [.plain.sha256]
```

Verify a bundle without Migration Assistant (unencrypted, or the ciphertext of an encrypted one):

```bash
cd <bundle> && for f in hashes/*.sha256; do case "$f" in *.plain.sha256) ;; *) sha256sum -c "$f" ;; esac; done
```
