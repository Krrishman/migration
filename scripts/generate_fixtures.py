#!/usr/bin/env python3
"""Generate deterministic fixture PCs for tests, mock mode and screenshots.

    python3 scripts/generate_fixtures.py

Creates:
  fixtures/source-pc/   a fake Windows 10 source PC (two users, browsers,
                        Outlook data, printers, drives, apps, secrets that
                        must never be captured)
  fixtures/target-pc/   a fake Windows 11 destination PC with existing data
                        (for collision tests)

Files named in SECRET_FILES contain the marker FIXTURE-SECRET-DO-NOT-COPY.
Tests assert that this marker never appears anywhere in a bundle.
"""
import json
import os
import shutil
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "fixtures"
SECRET = b"FIXTURE-SECRET-DO-NOT-COPY"
FIXED_MTIME = 1_700_000_000  # 2023-11-14, keeps fixtures deterministic


def write(base: Path, rel: str, data):
    p = base / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        data = data.encode("utf-8")
    p.write_bytes(data)
    os.utime(p, (FIXED_MTIME, FIXED_MTIME))
    return p


def chromium_bookmarks(names):
    def node(i, name, url):
        return {"date_added": "13300000000000000", "guid": f"00000000-0000-4000-8000-{i:012d}", "id": str(i), "name": name, "type": "url", "url": url}

    children = [node(i + 10, n, u) for i, (n, u) in enumerate(names)]
    return json.dumps(
        {
            "checksum": "fixture",
            "roots": {
                "bookmark_bar": {
                    "children": children[:2] + [{"children": children[2:], "id": "5", "name": "Work", "type": "folder", "date_added": "13300000000000000"}],
                    "id": "1",
                    "name": "Bookmarks bar",
                    "type": "folder",
                    "date_added": "13300000000000000",
                },
                "other": {"children": [], "id": "2", "name": "Other bookmarks", "type": "folder"},
                "synced": {"children": [], "id": "3", "name": "Mobile bookmarks", "type": "folder"},
            },
            "version": 1,
        },
        indent=2,
    )


def firefox_places(path: Path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        path.unlink()
    con = sqlite3.connect(path)
    con.executescript(
        """
        CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url LONGVARCHAR, title LONGVARCHAR, visit_count INTEGER DEFAULT 0, last_visit_date INTEGER);
        CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER DEFAULT NULL, parent INTEGER, position INTEGER, title LONGVARCHAR, dateAdded INTEGER, lastModified INTEGER, guid TEXT);
        INSERT INTO moz_places VALUES (1,'https://www.mozilla.org/','Mozilla',3,1700000000000000);
        INSERT INTO moz_places VALUES (2,'https://intranet.contoso.example/','Contoso Intranet',12,1700000000000000);
        INSERT INTO moz_places VALUES (3,'https://docs.contoso.example/handbook','Employee Handbook',1,1700000000000000);
        INSERT INTO moz_bookmarks VALUES (1,2,NULL,0,0,'',1700000000000000,1700000000000000,'root________');
        INSERT INTO moz_bookmarks VALUES (2,2,NULL,1,0,'menu',1700000000000000,1700000000000000,'menu________');
        INSERT INTO moz_bookmarks VALUES (3,2,NULL,1,1,'toolbar',1700000000000000,1700000000000000,'toolbar_____');
        INSERT INTO moz_bookmarks VALUES (10,1,1,3,0,'Mozilla',1700000000000000,1700000000000000,'bm10');
        INSERT INTO moz_bookmarks VALUES (11,2,NULL,3,1,'Work',1700000000000000,1700000000000000,'bm11');
        INSERT INTO moz_bookmarks VALUES (12,1,2,11,0,'Contoso Intranet',1700000000000000,1700000000000000,'bm12');
        INSERT INTO moz_bookmarks VALUES (13,1,3,11,1,'Employee Handbook <HR>',1700000000000000,1700000000000000,'bm13');
        """
    )
    con.commit()
    con.close()
    os.utime(path, (FIXED_MTIME, FIXED_MTIME))


def sticky_notes(path: Path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        path.unlink()
    con = sqlite3.connect(path)
    con.executescript("CREATE TABLE Note (Text TEXT, Id TEXT PRIMARY KEY); INSERT INTO Note VALUES ('Call IT about new laptop','n1');")
    con.commit()
    con.close()
    os.utime(path, (FIXED_MTIME, FIXED_MTIME))


def source_pc():
    base = ROOT / "source-pc"
    if base.exists():
        shutil.rmtree(base)
    ann = "Users/ann"
    w = lambda rel, data: write(base, rel, data)

    # System locations that must never be captured.
    w("Windows/System32/config/SAM", SECRET)
    w("Program Files/Contoso/app.exe", b"MZ fixture")
    w("ProgramData/Microsoft/Wlansvc/Profiles/Interfaces/{guid}/wifi.xml", b"<keyMaterial>" + SECRET + b"</keyMaterial>")

    # Ann (current user)
    w(f"{ann}/Documents/Report.docx", "Quarterly report draft\n" * 200)
    w(f"{ann}/Documents/Budget.xlsx", "budget,2024\n" * 500)
    w(f"{ann}/Documents/Projects/Migration/plan.txt", "1. Scan\n2. Capture\n3. Restore\n")
    w(f"{ann}/Documents/~$Report.docx", b"owner file (temporary)")
    w(f"{ann}/Documents/id_rsa", b"-----BEGIN OPENSSH PRIVATE KEY-----\n" + SECRET)
    w(f"{ann}/Documents/client-cert.pfx", SECRET)
    w(f"{ann}/Documents/Outlook Files/Archive 2022.pst", b"!BDN" + b"\0" * 4096)
    w(f"{ann}/Pictures/Wallpapers/aurora.jpg", b"\xff\xd8\xff\xe0fixture-jpeg" + b"\0" * 2048)
    w(f"{ann}/Pictures/Vacation/beach.jpg", b"\xff\xd8\xff\xe0beach" + b"\0" * 8192)
    w(f"{ann}/Downloads/installer.msi", b"\0" * 16384)
    w(f"{ann}/Videos/.keep", b"")
    w(f"{ann}/Music/playlist.m3u", "#EXTM3U\n")
    w(f"{ann}/Favorites/Links/Intranet.url", "[InternetShortcut]\nURL=https://intranet.contoso.example/\n")
    w(f"{ann}/Desktop/Notes.txt", "Remember to move the printer.\n")
    w(f"{ann}/Desktop/Budget shortcut.lnk", b"L\0\0\0fixture-shortcut")
    w(f"{ann}/NTUSER.DAT", SECRET)
    w(f"{ann}/.ssh/id_ed25519", SECRET)

    roaming = f"{ann}/AppData/Roaming"
    local = f"{ann}/AppData/Local"
    w(f"{roaming}/Microsoft/Signatures/Work.htm", "<html><body><p>Ann Example<br>Contoso Ltd.</p><img src=\"Work_files/image001.png\"></body></html>")
    w(f"{roaming}/Microsoft/Signatures/Work.txt", "Ann Example\nContoso Ltd.\n")
    w(f"{roaming}/Microsoft/Signatures/Work_files/image001.png", b"\x89PNG\r\n\x1a\nfixture")
    w(f"{roaming}/Microsoft/Templates/Normal.dotm", b"PK\x03\x04normal-template")
    w(f"{roaming}/Microsoft/Templates/Weekly status.oft", b"\xd0\xcf\x11\xe0outlook-template")
    w(f"{roaming}/Microsoft/Templates/Letterhead.dotx", b"PK\x03\x04letterhead")
    w(f"{roaming}/Microsoft/Stationery/Contoso.htm", "<html><body style='font-family:Segoe UI'></body></html>")
    w(f"{roaming}/Microsoft/UProof/CUSTOM.DIC", "Contoso\nMigrationAssistant\n")
    w(f"{roaming}/Microsoft/Credentials/DFBE70A7E5CC19A398EBF1B96859CE5D", SECRET)
    w(f"{roaming}/Microsoft/Protect/S-1-5-21-1000/masterkey", SECRET)
    w(f"{roaming}/Microsoft/Windows/Start Menu/Programs/Contoso Tool.lnk", b"L\0\0\0start-menu")
    w(f"{roaming}/Microsoft/Windows/Recent/Report.docx.lnk", b"L\0\0\0recent")
    w(f"{roaming}/Microsoft/Windows/Recent/AutomaticDestinations/f01b4d95cf55d32a.automaticDestinations-ms", b"\xd0\xcf\x11\xe0quick-access")
    w(f"{roaming}/Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar/File Explorer.lnk", b"L\0\0\0taskbar")

    chrome = f"{local}/Google/Chrome/User Data"
    w(f"{chrome}/Local State", json.dumps({"os_crypt": {"encrypted_key": SECRET.decode()}, "profile": {"info_cache": {"Default": {"name": "Ann (Work)"}}}}))
    w(f"{chrome}/Default/Preferences", json.dumps({"profile": {"name": "Ann (Work)"}, "homepage": "https://intranet.contoso.example/"}))
    w(f"{chrome}/Default/Bookmarks", chromium_bookmarks([("Contoso Intranet", "https://intranet.contoso.example/"), ("Microsoft Learn", "https://learn.microsoft.com/"), ("Expense <Portal> & Travel", "https://expenses.contoso.example/?a=1&b=2")]))
    w(f"{chrome}/Default/History", b"SQLite format 3\0fixture-history" + b"\0" * 1024)
    w(f"{chrome}/Default/Favicons", b"SQLite format 3\0favicons")
    w(f"{chrome}/Default/Login Data", SECRET)
    w(f"{chrome}/Default/Cookies", SECRET)
    w(f"{chrome}/Default/Network/Cookies", SECRET)
    w(f"{chrome}/Default/Web Data", SECRET)
    w(f"{chrome}/Default/Secure Preferences", SECRET)
    w(f"{chrome}/Default/Local Storage/leveldb/000003.log", SECRET)
    w(f"{chrome}/Default/Sessions/Session_1", SECRET)
    w(f"{chrome}/Default/Cache/Cache_Data/data_0", b"cache" * 100)
    w(f"{chrome}/Default/Extensions/aapocclcgogkmnckokdopfmhonfmgoek/1.0_0/manifest.json", json.dumps({"name": "Docs", "version": "1.0"}))
    w(f"{chrome}/Profile 1/Preferences", json.dumps({"profile": {"name": "Personal"}}))
    w(f"{chrome}/Profile 1/Bookmarks", chromium_bookmarks([("News", "https://news.example/")]))
    w(f"{chrome}/Profile 1/Login Data", SECRET)

    edge = f"{local}/Microsoft/Edge/User Data"
    w(f"{edge}/Default/Preferences", json.dumps({"profile": {"name": "Profile 1"}}))
    w(f"{edge}/Default/Bookmarks", chromium_bookmarks([("Contoso SharePoint", "https://contoso.sharepoint.example/")]))
    w(f"{edge}/Default/Login Data For Account", SECRET)

    ff = f"{roaming}/Mozilla/Firefox"
    w(f"{ff}/profiles.ini", "[Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/x7k2.default-release\nDefault=1\n\n[General]\nStartWithLastProfile=1\nVersion=2\n")
    firefox_places(base / f"{ff}/Profiles/x7k2.default-release/places.sqlite")
    w(f"{ff}/Profiles/x7k2.default-release/prefs.js", 'user_pref("browser.startup.homepage", "https://intranet.contoso.example/");\n')
    w(f"{ff}/Profiles/x7k2.default-release/logins.json", SECRET)
    w(f"{ff}/Profiles/x7k2.default-release/key4.db", SECRET)
    w(f"{ff}/Profiles/x7k2.default-release/cookies.sqlite", SECRET)
    w(f"{ff}/Profiles/x7k2.default-release/cert9.db", SECRET)

    w(f"{local}/Microsoft/Outlook/ann@contoso.example.ost", b"!BDN" + b"\0" * 2048)
    w(f"{local}/Microsoft/Windows/Themes/Contoso.theme", "[Theme]\nDisplayName=Contoso\n[Control Panel\\Desktop]\nWallpaper=%USERPROFILE%\\Pictures\\Wallpapers\\aurora.jpg\n[Slideshow]\nImagesRootPath=%USERPROFILE%\\Pictures\\Wallpapers\n")
    sticky_notes(base / f"{local}/Packages/Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe/LocalState/plum.sqlite")
    w(f"{local}/Temp/setup-log.tmp", b"temp")
    w(f"{local}/Microsoft/Vault/4BF4C442/policy.vpol", SECRET)

    # Bob (another local user)
    w("Users/bob/Documents/bob-notes.txt", "Bob's notes\n")
    w("Users/bob/Desktop/todo.txt", "todo\n")
    # Locked-down user (simulated access denied)
    (base / "Users/svc-backup").mkdir(parents=True, exist_ok=True)
    w("Users/Public/Desktop/Company Portal.url", "[InternetShortcut]\nURL=https://portal.contoso.example/\n")

    spec = {
        "machine": {
            "computer_name": "ACCT-PC-07",
            "os_name": "Windows 10 Pro",
            "os_version": "22H2 (Professional)",
            "os_build": "19045.4529",
            "architecture": "AMD64",
            "cpu_summary": "Intel(R) Core(TM) i5-8500 CPU @ 3.00GHz",
            "total_memory_bytes": 17179869184,
            "time_zone": "Pacific Standard Time",
            "join_state": "domain",
            "join_name": "CONTOSO",
        },
        "elevated": False,
        "profiles": [
            {"sid": "S-1-5-18", "account_name": "NT AUTHORITY\\SYSTEM", "profile_dir": "Windows/System32/config/systemprofile"},
            {"sid": "S-1-5-21-1004336348-1177238915-682003330-1104", "account_name": "CONTOSO\\ann", "display_name": "Ann Example", "profile_dir": "Users/ann", "is_current_user": True, "last_use": "2024-06-03T08:15:00Z"},
            {"sid": "S-1-5-21-1004336348-1177238915-682003330-1187", "account_name": "CONTOSO\\bob", "display_name": "Bob Sample", "profile_dir": "Users/bob", "last_use": "2024-02-11T16:40:00Z"},
            {"sid": "S-1-5-21-1004336348-1177238915-682003330-1203", "account_name": "CONTOSO\\svc-backup", "profile_dir": "Users/svc-backup", "access_denied": True},
        ],
        "public_desktop": "Users/Public/Desktop",
        "system_roots": ["Windows", "Program Files", "Program Files (x86)"],
        "wallpapers": {"S-1-5-21-1004336348-1177238915-682003330-1104": "Users/ann/Pictures/Wallpapers/aurora.jpg"},
        "outlook_profiles": {"S-1-5-21-1004336348-1177238915-682003330-1104": ["Outlook", "Contoso Shared Mailbox"]},
        "processes": ["explorer.exe", "msedge.exe", "Teams.exe"],
        "long_paths_enabled": False,
        "installed_apps": [
            {"display_name": "Google Chrome", "version": "125.0.6422.142", "publisher": "Google LLC", "install_location": "C:\\Program Files\\Google\\Chrome\\Application", "install_date": "20240520", "uninstall_command": "\"C:\\Program Files\\Google\\Chrome\\Application\\125.0.6422.142\\Installer\\setup.exe\" --uninstall --system-level", "architecture": "x64", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "Microsoft 365 Apps for enterprise - en-us", "version": "16.0.17531.20152", "publisher": "Microsoft Corporation", "install_location": "C:\\Program Files\\Microsoft Office", "install_date": None, "uninstall_command": "\"C:\\Program Files\\Common Files\\Microsoft Shared\\ClickToRun\\OfficeClickToRun.exe\" scenario=install", "architecture": "x64", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "Mozilla Firefox (x64 en-US)", "version": "126.0.1", "publisher": "Mozilla", "install_location": "C:\\Program Files\\Mozilla Firefox", "install_date": None, "uninstall_command": "\"C:\\Program Files\\Mozilla Firefox\\uninstall\\helper.exe\"", "architecture": "x64", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "7-Zip 23.01 (x64)", "version": "23.01", "publisher": "Igor Pavlov", "install_location": "C:\\Program Files\\7-Zip\\", "install_date": "20231102", "uninstall_command": "\"C:\\Program Files\\7-Zip\\Uninstall.exe\"", "architecture": "x64", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "Contoso Expense Client", "version": "4.2.0", "publisher": "Contoso Ltd.", "install_location": None, "install_date": "20220915", "uninstall_command": "MsiExec.exe /X{6C2B0B8E-1111-2222-3333-444455556666}", "architecture": "x86", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "Microsoft Visual C++ 2015-2022 Redistributable (x64) - 14.38.33135", "version": "14.38.33135.0", "publisher": "Microsoft Corporation", "install_location": None, "install_date": "20240110", "uninstall_command": None, "architecture": "x64", "scope": "Machine", "category": "", "description": None, "settings_plugin": None},
            {"display_name": "Zoom Workplace", "version": "6.0.11", "publisher": "Zoom Video Communications, Inc.", "install_location": None, "install_date": "20240601", "uninstall_command": None, "architecture": "x64", "scope": "Current user", "category": "", "description": None, "settings_plugin": None},
        ],
        "printers": [
            {"name": "HP LaserJet 4th Floor", "share_name": None, "port_name": "IP_10.20.4.25", "port_type": "Standard TCP/IP Port", "host_address": "10.20.4.25", "unc_path": None, "connection": "network", "driver_name": "HP Universal Printing PCL 6", "driver_version": "61.250.1.24832", "is_default": True, "status": "Normal"},
            {"name": "\\\\printsrv01\\Finance-Color", "share_name": "Finance-Color", "port_name": "\\\\printsrv01\\Finance-Color", "port_type": None, "host_address": None, "unc_path": "\\\\printsrv01\\Finance-Color", "connection": "shared", "driver_name": "Xerox Global Print Driver PCL6", "driver_version": "5.887.3.0", "is_default": False, "status": "Normal"},
            {"name": "Brother HL-L2350DW", "share_name": None, "port_name": "USB001", "port_type": "Local Port", "host_address": None, "unc_path": None, "connection": "usb", "driver_name": "Brother HL-L2350D series", "driver_version": None, "is_default": False, "status": "Offline"},
            {"name": "Microsoft Print to PDF", "share_name": None, "port_name": "PORTPROMPT:", "port_type": "Local Port", "host_address": None, "unc_path": None, "connection": "virtual", "driver_name": "Microsoft Print To PDF", "driver_version": None, "is_default": False, "status": "Normal"},
        ],
        "printer_drivers": ["HP Universal Printing PCL 6", "Microsoft Print To PDF"],
        "mapped_drives": [
            {"letter": "H:", "unc_path": "\\\\fs01.contoso.example\\home$\\ann", "provider": "Microsoft Windows Network", "persistent": True, "status": "Connected", "label": "Home"},
            {"letter": "S:", "unc_path": "\\\\fs01.contoso.example\\shared", "provider": "Microsoft Windows Network", "persistent": True, "status": "Disconnected", "label": "Shared"},
        ],
    }
    (base / "platform.json").write_text(json.dumps(spec, indent=2) + "\n")


def target_pc():
    base = ROOT / "target-pc"
    if base.exists():
        shutil.rmtree(base)
    w = lambda rel, data: write(base, rel, data)
    w("Users/annexample/Documents/Report.docx", "Existing destination copy - must not be overwritten silently\n")
    w("Users/annexample/Desktop/Welcome.txt", "Welcome to your new PC\n")
    w("Users/annexample/Pictures/.keep", b"")
    w("Users/annexample/AppData/Local/Google/Chrome/User Data/Default/Preferences", json.dumps({"profile": {"name": "Person 1"}}))
    w("Users/annexample/AppData/Local/Google/Chrome/User Data/Default/Bookmarks", chromium_bookmarks([("New PC bookmark", "https://example.org/")]))
    (base / "Users/Public/Desktop").mkdir(parents=True, exist_ok=True)
    (base / "Windows").mkdir(parents=True, exist_ok=True)
    spec = {
        "machine": {
            "computer_name": "ACCT-PC-21",
            "os_name": "Windows 11 Pro",
            "os_version": "23H2 (Professional)",
            "os_build": "22631.3737",
            "architecture": "AMD64",
            "time_zone": "Pacific Standard Time",
            "join_state": "azure_ad",
        },
        "elevated": False,
        "profiles": [
            {"sid": "S-1-12-1-3456789012-1234567890-987654321-1001", "account_name": "AzureAD\\AnnExample", "display_name": "Ann Example", "profile_dir": "Users/annexample", "is_current_user": True},
        ],
        "public_desktop": "Users/Public/Desktop",
        "system_roots": ["Windows", "Program Files", "Program Files (x86)"],
        "printer_drivers": ["HP Universal Printing PCL 6"],
        "processes": ["explorer.exe"],
    }
    (base / "platform.json").write_text(json.dumps(spec, indent=2) + "\n")


if __name__ == "__main__":
    source_pc()
    target_pc()
    print(f"Fixtures written to {ROOT}")
