# Privacy

Migration Assistant processes personal data **only on the computer where it runs** and only so the technician can move it to another computer. It has no telemetry, analytics, crash reporting, update checks, cloud services or network connections of any kind.

## What is read during a scan

| Data | Why | Stored in bundle? |
|---|---|---|
| Computer name, Windows edition/version/build, architecture, time zone, domain/workgroup/Azure AD indicator | Compatibility and reports | Yes, if "Computer inventory" is selected (`inventory/machine.json`, manifest `source_machine`) |
| CPU and RAM summary, drive sizes and free space, network adapter names and MAC addresses | Planning | Yes, with the computer inventory. IP addresses are not collected. |
| Local profiles: account name, SID, profile path, last use (with confidence) | User mapping | Yes, for selected users. The profile list is stored only if selected. |
| File and folder names and sizes in reviewed locations | Size estimates | Only the selected items' files are copied |
| Browser profile folder names and display names | Choosing profiles | Yes (names only) |
| Outlook profile **names**, signature/template/PST/OST locations | Outlook guidance | Yes (names and locations only) |
| Printer, mapped drive and installed application inventories | Restore checklists | Yes, if selected. Uninstall commands appear only in the technician report and are marked sensitive. |
| Running process names | Detecting open browsers, Outlook and Sticky Notes | No (preflight only) |

No device identifier is read; the manifest field `device_id` is always empty and shown as "Not collected".

## What is never collected

Passwords and saved browser logins, cookies, session and authentication tokens, autofill and payment data, Windows Credential Manager entries, DPAPI keys, Wi-Fi passwords, private keys and certificates with keys, SSH/GPG keys, password-manager databases, license and product keys, registry hives and program files.

## Privacy-sensitive items

- **Recent items**, the shortcuts to recently opened files, reveal user activity. They are labelled privacy-sensitive, never preselected, and selectable only after the technician enables "Allow privacy-sensitive items".
- **Other users' data** is never preselected, even when it is readable.
- **Last-use times** are shown for planning only, with a confidence label and a privacy notice.

## Where data goes

- The bundle is written only to the destination the technician selects, under `<destination>\migrations\`.
- App data (the authorization acknowledgement, plus an index of bundle paths, IDs and statuses) is kept beside the executable in `MigrationAssistantData\`, and only if that folder is writable. Nothing is written to AppData.
- On restore, data goes only into the mapped destination profiles. The restore report is stored in the bundle (or the target user's Documents if the bundle is read-only).

## Reports

- **Technician report** (`report.html`, `report.json`) is detailed and should stay local.
- **Summary report** (`summary-report.html`) is redacted: account names, profile paths, the computer name, the domain, UNC server names and e-mail addresses are replaced with placeholders. It is suitable for sharing with the end user or a ticket.

## Retention and deletion

Migration Assistant does not upload or retain data elsewhere. Delete the bundle folder from the destination drive when the migration is accepted. Incomplete bundles can be deleted from the completion screen after typing the bundle ID. Use bundle encryption whenever a drive may leave your control.

## Authorization

Before the first use (and before each scan), the technician must confirm that the device owner and the affected users authorized the migration. The tool works only locally, so authorization is the technician's responsibility under their organization's policy.
