// Browser-preview backend. Replays data produced by a real fixture run
// (src/mocks/data, built by scripts/build_mock_data.py) and simulates
// progress events. It never touches the file system and is always labelled
// "Demo data" in the UI.

import type { Backend, Unlisten } from "../lib/api";
import type {
  AppStatus,
  BundleOverview,
  CaptureRequest,
  CaptureSummary,
  DiscoveryItem,
  DiskSpace,
  IndexedBundle,
  LogEntry,
  PreflightReport,
  ReportView,
  RestorePlan,
  RestoreRequest,
  RestoreSummary,
  ScanProgress,
  ScanResult,
  TargetInfo,
  TaskProgress,
} from "../lib/types";
import scanData from "./data/scan.json";
import captureData from "./data/capture-summary.json";
import overviewData from "./data/overview.json";
import planData from "./data/plan.json";
import restoreData from "./data/restore-summary.json";

const GB = 1024 ** 3;
const scanFixture = scanData as unknown as ScanResult;
const captureFixture = captureData as unknown as CaptureSummary;
const overviewFixture = overviewData as unknown as BundleOverview;
const planFixture = planData as unknown as RestorePlan;
const restoreFixture = restoreData as unknown as RestoreSummary;

type Listener = (payload: unknown) => void;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class MockBackend implements Backend {
  readonly mode = "mock" as const;
  private listeners = new Map<string, Set<Listener>>();
  private acknowledged = false;
  private destination: string | null = "E:\\";
  private scan: ScanResult | null = null;
  private canceled = false;
  /** Speed multiplier for tests. */
  speed = 1;

  private emit(event: string, payload: unknown) {
    this.listeners.get(event)?.forEach((cb) => cb(payload));
  }

  async on<T>(event: string, cb: (payload: T) => void): Promise<Unlisten> {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    const set = this.listeners.get(event)!;
    const l = cb as Listener;
    set.add(l);
    return () => set.delete(l);
  }

  async appStatus(): Promise<AppStatus> {
    return {
      app_version: "0.1.0",
      platform: "mock:browser-preview",
      fixture_mode: true,
      elevated: false,
      computer_name: scanFixture.machine.computer_name,
      paths: {
        exe_path: "E:\\MigrationAssistant\\MigrationAssistant.exe",
        exe_dir: "E:\\MigrationAssistant",
        exe_dir_writable: true,
        data_dir: "E:\\MigrationAssistant\\MigrationAssistantData",
        default_destination: "E:\\",
        config_file: "E:\\MigrationAssistant\\migration-assistant.config.json",
        config_loaded: false,
      },
      authorization_acknowledged: this.acknowledged,
      session_destination: this.destination,
      network_access: "none",
    };
  }
  async acknowledgeAuthorization() {
    this.acknowledged = true;
  }
  async setDestination(path: string) {
    if (/^C:\\(Windows|Program Files)/i.test(path)) throw "Choose a destination outside Windows and Program Files.";
    this.destination = path;
    return path;
  }
  async saveConfig() {
    return "E:\\MigrationAssistant\\migration-assistant.config.json";
  }
  async diskSpace(path: string): Promise<DiskSpace | null> {
    const usb = /^[D-Z]:/i.test(path);
    return { path, total_bytes: usb ? 256 * GB : 476 * GB, free_bytes: usb ? 118.4 * GB : 141.7 * GB };
  }
  async restartElevated() {
    throw "Demo mode: elevation is simulated. In the desktop app Windows shows a UAC prompt.";
  }
  async openPath(path: string) {
    console.info("[demo] open", path);
  }
  async listBundles(): Promise<IndexedBundle[]> {
    return [
      {
        bundle_id: captureFixture.bundle_id,
        path: captureFixture.bundle_path,
        computer_name: captureFixture.machine_name,
        created_at: overviewFixture.manifest.created_at,
        status: "Verified",
        last_event: "capture",
        updated_at: overviewFixture.manifest.created_at,
      },
    ];
  }
  async loadReport(): Promise<ReportView> {
    return {
      kind: "bundle",
      manifest: overviewFixture.manifest,
      html_path: captureFixture.report_html,
      summary_html_path: captureFixture.summary_report_html,
      bundle_path: captureFixture.bundle_path,
    };
  }
  async pickFolder(title: string) {
    if (/bundle/i.test(title)) return captureFixture.bundle_path;
    if (/browser/i.test(title)) return "C:\\Users\\ann\\AppData\\Local\\BraveSoftware\\Brave-Browser\\User Data";
    if (/folder to include|custom/i.test(title)) return "D:\\Projects";
    return "E:\\";
  }
  async pickFile() {
    return `${captureFixture.bundle_path}\\manifest.json`;
  }

  async startScan(): Promise<ScanResult> {
    this.canceled = false;
    const stages = [
      "Reading computer information",
      "Enumerating local user profiles",
      "Collecting system inventory",
      "Checking user profiles",
      "Measuring user folders",
      "Scanning desktops and shortcuts",
      "Discovering browser profiles",
      "Discovering Outlook data",
      "Reading personalization",
      "Reading printer inventory",
      "Reading mapped network drives",
      "Building application inventory",
      "Checking application settings plug-ins",
    ];
    for (let i = 0; i < stages.length; i++) {
      if (this.canceled) throw "Operation canceled";
      this.emit("scan://progress", { stage: `s${i}`, stage_index: i, stage_count: stages.length, message: stages[i] } satisfies ScanProgress);
      await sleep(180 / this.speed);
    }
    this.scan = structuredClone(scanFixture);
    return this.scan;
  }
  async cancelScan() {
    this.canceled = true;
    return true;
  }
  async getScan() {
    return this.scan;
  }
  async addCustomFolder(path: string, ownerSid: string | null): Promise<DiscoveryItem> {
    if (/^C:\\?$|^C:\\(Windows|Program Files)/i.test(path)) throw `Path rejected by safety rules: ${path}: Operating system or program files are never migrated`;
    const owner = this.scan?.users.find((u) => u.sid === ownerSid);
    const item: DiscoveryItem = {
      id: `custom:${ownerSid ?? "machine"}:${path.toLowerCase().replace(/[^a-z0-9]/g, "_")}`,
      category: "users_files",
      display_name: `Custom folder: ${path.split("\\").filter(Boolean).pop()}`,
      description: "Folder added by the technician. Copied recursively with standard exclusions.",
      source: { kind: "path", path },
      owner: owner ? { sid: owner.sid, account_name: owner.account_name } : undefined,
      estimated_size: 3.2 * GB,
      item_count: 18342,
      access: "accessible",
      support: "supported",
      selected_by_default: true,
      sensitive: false,
      opt_in_only: false,
      requires_admin: false,
      warnings: [],
      restore_notes: ["Restored to \"Migrated Files\" in the mapped user's profile."],
      includes: ["All files and subfolders"],
      excludes: ["Temporary files, recycle bins, links/junctions, protected credential and key files"],
      restore_kind: { type: "custom_folder" },
    };
    this.scan?.items.push(item);
    return item;
  }
  async addChromiumRoot(): Promise<DiscoveryItem[]> {
    throw "Demo mode: no Chromium \"User Data\" folder was found at that location.";
  }

  async capturePreflight(request: CaptureRequest): Promise<PreflightReport> {
    const items = (this.scan?.items ?? []).filter((i) => request.selected_item_ids.includes(i.id));
    const estimated = items.reduce((a, i) => a + (i.estimated_size ?? 0), 0);
    const free = 118.4 * GB;
    const blocking: string[] = [];
    if (!items.length) blocking.push("Nothing is selected.");
    if (request.encryption.enabled && (request.encryption.passphrase?.length ?? 0) < 12) blocking.push("Encryption is enabled but no valid passphrase (at least 12 characters) was provided.");
    const running = items.some((i) => i.category === "browsers" && i.display_name.includes("Edge")) ? ["Microsoft Edge"] : [];
    return {
      destination_root: request.destination_root,
      destination_writable: true,
      destination_free_bytes: free,
      source_free_bytes: 141.7 * GB,
      estimated_bytes: estimated,
      unknown_size_items: 0,
      sufficient_space: free > estimated,
      long_paths_enabled: false,
      destination_file_system: "exFAT",
      selected_count: items.length,
      running_apps: running,
      warnings: [
        ...running.map((r) => ({ code: "application_running", severity: "warning" as const, message: `${r} is running. Close it, then choose Retry detection. Locked files are skipped (and reported) if you continue.` })),
        { code: "long_path", severity: "info" as const, message: "Long path support is disabled in Windows. Migration Assistant still copies long paths, but Explorer may not open them." },
      ],
      blocking_errors: blocking,
    };
  }

  private async simulate(channel: "capture" | "restore", tasks: TaskProgress[]): Promise<boolean> {
    this.canceled = false;
    const live: TaskProgress[] = tasks.map((t) => ({ ...t, state: "queued", bytes_done: 0, items_done: 0, elapsed_ms: 0, eta_seconds: null, bytes_per_second: null }));
    live.forEach((t) => this.emit(`${channel}://progress`, t));
    const log = (level: LogEntry["level"], message: string, task_id: string | null = null) =>
      this.emit(`${channel}://log`, { timestamp: new Date().toISOString(), level, task_id, message } satisfies LogEntry);
    log("info", `${channel === "capture" ? "Starting capture" : "Starting restore"} (demo data)`);
    for (let i = 0; i < live.length; i++) {
      const final = tasks[i];
      const t = live[i];
      const steps = Math.max(3, Math.min(10, Math.ceil((final.bytes_total ?? final.bytes_done) / (300 * 1024 * 1024))));
      for (const phase of ["scanning", "copying"] as const) {
        if (this.canceled) {
          for (let j = i; j < live.length; j++) this.emit(`${channel}://progress`, { ...live[j], state: "canceled" });
          log("warning", "Canceled by the technician; completed work is kept.");
          return false;
        }
        t.state = phase;
        this.emit(`${channel}://progress`, { ...t });
        await sleep(90 / this.speed);
      }
      for (let s = 1; s <= steps; s++) {
        if (this.canceled) break;
        const frac = s / steps;
        t.bytes_done = Math.round(final.bytes_done * frac);
        t.items_done = Math.round(final.items_done * frac);
        t.elapsed_ms += 400;
        const rate = 86 * 1024 * 1024;
        t.bytes_per_second = rate;
        t.eta_seconds = s > 2 && final.bytes_total ? Math.ceil((final.bytes_total - t.bytes_done) / rate) : null;
        t.current_path = final.display_name.includes("Documents") ? "C:\\Users\\ann\\Documents\\Projects\\Migration\\plan.txt" : final.current_path;
        this.emit(`${channel}://progress`, { ...t });
        await sleep(110 / this.speed);
      }
      t.state = "verifying";
      this.emit(`${channel}://progress`, { ...t });
      await sleep(80 / this.speed);
      Object.assign(t, { ...final, current_path: null, eta_seconds: null });
      this.emit(`${channel}://progress`, { ...t });
      if (final.warning_count) log("warning", `${final.display_name}: ${final.warning_count} warning(s)`, final.task_id);
      else log("info", `${final.display_name}: done`, final.task_id);
    }
    return true;
  }

  async startCapture(request: CaptureRequest): Promise<CaptureSummary> {
    const known = new Set(captureFixture.task_results.map((t) => t.task_id));
    const tasks = captureFixture.task_results.filter((t) => known.has(t.task_id) && (request.selected_item_ids.includes(t.task_id) || t.task_id.startsWith("inventory:")));
    const ok = await this.simulate("capture", tasks.length ? tasks : captureFixture.task_results);
    if (!ok) return { ...captureFixture, status: "canceled", verified: false, task_results: captureFixture.task_results.map((t) => ({ ...t, state: "canceled" })) };
    return captureFixture;
  }
  async cancelCapture() {
    this.canceled = true;
    return true;
  }
  async resumeCapture() {
    await this.simulate("capture", captureFixture.task_results.slice(-3));
    return captureFixture;
  }
  async deleteIncompleteBundle(_bundlePath: string, confirmBundleId: string) {
    if (confirmBundleId !== captureFixture.bundle_id) throw "Type the bundle ID exactly to confirm deletion.";
  }
  async prepareRestoreInstructions(bundlePath: string) {
    return `${bundlePath}\\RESTORE-INSTRUCTIONS.txt`;
  }

  async openBundle(): Promise<BundleOverview> {
    for (let n = 1; n <= 40; n += 7) {
      this.emit("restore://verify", [n, `users/ann/files/Documents/file-${n}`]);
      await sleep(60 / this.speed);
    }
    return structuredClone(overviewFixture);
  }
  async targetInfo(): Promise<TargetInfo> {
    return overviewFixture.target;
  }
  async planRestore(request: RestoreRequest): Promise<RestorePlan> {
    await sleep(150 / this.speed);
    const plan = structuredClone(planFixture);
    const mappedSources = new Set(request.mappings.filter((m) => m.target_sid).map((m) => m.source_sid));
    plan.actions = plan.actions
      .filter((a) => request.selected_item_ids.includes(a.item_id))
      .filter((a) => {
        const item = overviewFixture.manifest.items.find((i) => i.id === a.item_id);
        return !item?.owner || mappedSources.has(item.owner.sid);
      })
      .map((a) => ({ ...a, policy: a.id.startsWith("bookmarks:") ? a.policy : request.policies[a.category] ?? "skip_existing" }));
    plan.total_files = plan.actions.reduce((s, a) => s + a.files, 0);
    plan.total_conflicts = plan.actions.reduce((s, a) => s + a.conflicts, 0);
    plan.total_bytes = plan.actions.filter((a) => !a.blocked_reason).reduce((s, a) => s + a.bytes, 0);
    return plan;
  }
  async executeRestore(request: RestoreRequest): Promise<RestoreSummary> {
    const tasks = restoreFixture.task_results.filter((t) => {
      const a = planFixture.actions.find((x) => x.id === t.task_id);
      return a ? request.selected_item_ids.includes(a.item_id) : true;
    });
    await this.simulate("restore", tasks);
    return { ...restoreFixture, task_results: tasks.map((t) => (request.confirmed_categories.includes(t.category) || t.state === "skipped" ? t : { ...t, state: "skipped", error: "Not confirmed" })) };
  }
  async cancelRestore() {
    this.canceled = true;
    return true;
  }
}
