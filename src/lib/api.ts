// Backend abstraction. In the desktop app every call goes to a typed Rust
// command over Tauri IPC. In a plain browser (npm run dev without Tauri) a
// clearly labelled mock backend replays real fixture data so the UI can be
// developed and tested without Windows or administrator rights.

import type {
  AppStatus,
  BundleOverview,
  CaptureRequest,
  CaptureSummary,
  DiscoveryItem,
  DiskSpace,
  IndexedBundle,
  PreflightReport,
  ReportView,
  RestorePlan,
  RestoreRequest,
  RestoreSummary,
  ScanResult,
  TargetInfo,
} from "./types";

export type Unlisten = () => void;

export interface Backend {
  readonly mode: "tauri" | "mock";
  appStatus(): Promise<AppStatus>;
  acknowledgeAuthorization(): Promise<void>;
  setDestination(path: string): Promise<string>;
  saveConfig(): Promise<string>;
  diskSpace(path: string): Promise<DiskSpace | null>;
  restartElevated(modules: string[]): Promise<void>;
  openPath(path: string): Promise<void>;
  listBundles(): Promise<IndexedBundle[]>;
  loadReport(path: string): Promise<ReportView>;
  pickFolder(title: string): Promise<string | null>;
  pickFile(title: string, extensions: string[]): Promise<string | null>;

  startScan(): Promise<ScanResult>;
  cancelScan(): Promise<boolean>;
  getScan(): Promise<ScanResult | null>;
  addCustomFolder(path: string, ownerSid: string | null): Promise<DiscoveryItem>;
  addChromiumRoot(path: string, name: string, ownerSid: string): Promise<DiscoveryItem[]>;

  capturePreflight(request: CaptureRequest): Promise<PreflightReport>;
  startCapture(request: CaptureRequest): Promise<CaptureSummary>;
  cancelCapture(): Promise<boolean>;
  resumeCapture(bundlePath: string, passphrase: string | null): Promise<CaptureSummary>;
  deleteIncompleteBundle(bundlePath: string, confirmBundleId: string): Promise<void>;
  prepareRestoreInstructions(bundlePath: string): Promise<string>;

  openBundle(path: string): Promise<BundleOverview>;
  targetInfo(): Promise<TargetInfo>;
  planRestore(request: RestoreRequest): Promise<RestorePlan>;
  executeRestore(request: RestoreRequest): Promise<RestoreSummary>;
  cancelRestore(): Promise<boolean>;

  on<T>(event: string, cb: (payload: T) => void): Promise<Unlisten>;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Normalize errors from IPC (strings) and JS (Error) into a message. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

class TauriBackend implements Backend {
  readonly mode = "tauri" as const;

  private async call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }

  appStatus() { return this.call<AppStatus>("app_status"); }
  acknowledgeAuthorization() { return this.call<void>("acknowledge_authorization"); }
  setDestination(path: string) { return this.call<string>("set_destination", { path }); }
  saveConfig() { return this.call<string>("save_config"); }
  diskSpace(path: string) { return this.call<DiskSpace | null>("disk_space", { path }); }
  restartElevated(modules: string[]) { return this.call<void>("restart_elevated", { modules }); }
  openPath(path: string) { return this.call<void>("open_path", { path }); }
  listBundles() { return this.call<IndexedBundle[]>("list_bundles"); }
  loadReport(path: string) { return this.call<ReportView>("load_report", { path }); }

  async pickFolder(title: string) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: true, multiple: false, title });
    return typeof r === "string" ? r : null;
  }
  async pickFile(title: string, extensions: string[]) {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const r = await open({ directory: false, multiple: false, title, filters: [{ name: extensions.join(", "), extensions }] });
    return typeof r === "string" ? r : null;
  }

  startScan() { return this.call<ScanResult>("start_scan"); }
  cancelScan() { return this.call<boolean>("cancel_scan"); }
  getScan() { return this.call<ScanResult | null>("get_scan"); }
  addCustomFolder(path: string, ownerSid: string | null) { return this.call<DiscoveryItem>("add_custom_folder", { path, ownerSid }); }
  addChromiumRoot(path: string, name: string, ownerSid: string) { return this.call<DiscoveryItem[]>("add_chromium_root", { path, name, ownerSid }); }

  capturePreflight(request: CaptureRequest) { return this.call<PreflightReport>("capture_preflight", { request }); }
  startCapture(request: CaptureRequest) { return this.call<CaptureSummary>("start_capture", { request }); }
  cancelCapture() { return this.call<boolean>("cancel_capture"); }
  resumeCapture(bundlePath: string, passphrase: string | null) { return this.call<CaptureSummary>("resume_capture", { bundlePath, passphrase }); }
  deleteIncompleteBundle(bundlePath: string, confirmBundleId: string) { return this.call<void>("delete_incomplete_bundle", { bundlePath, confirmBundleId }); }
  prepareRestoreInstructions(bundlePath: string) { return this.call<string>("prepare_restore_instructions", { bundlePath }); }

  openBundle(path: string) { return this.call<BundleOverview>("open_bundle", { path }); }
  targetInfo() { return this.call<TargetInfo>("target_info"); }
  planRestore(request: RestoreRequest) { return this.call<RestorePlan>("plan_restore", { request }); }
  executeRestore(request: RestoreRequest) { return this.call<RestoreSummary>("execute_restore", { request }); }
  cancelRestore() { return this.call<boolean>("cancel_restore"); }

  async on<T>(event: string, cb: (payload: T) => void): Promise<Unlisten> {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (e) => cb(e.payload));
  }
}

let instance: Backend | null = null;

export async function getBackend(): Promise<Backend> {
  if (instance) return instance;
  if (isTauri()) {
    instance = new TauriBackend();
  } else {
    const { MockBackend } = await import("../mocks/mockBackend");
    instance = new MockBackend();
  }
  return instance;
}

/** Test hook. */
export function setBackend(b: Backend | null) {
  instance = b;
}
