// TypeScript mirrors of the Rust domain models (serde snake_case).
// Keep in sync with src-tauri/src/models/*.rs.

export type Category =
  | "users_files"
  | "browsers"
  | "outlook_email"
  | "personalization"
  | "printers"
  | "network_drives"
  | "desktop_shortcuts"
  | "application_settings"
  | "installed_applications"
  | "system_inventory";

export type SupportLevel = "supported" | "partial" | "inventory_only" | "unsupported";
export type AccessState = "accessible" | "partially_accessible" | "access_denied" | "locked" | "not_found" | "unknown";
export type Severity = "info" | "warning" | "error";

export interface Warning {
  code: string;
  severity: Severity;
  message: string;
  path?: string;
}

export interface UserRef {
  sid: string;
  account_name: string;
}

export type SourceRef = { kind: "path"; path: string } | { kind: "config"; reference: string };
export type BrowserKind = "chrome" | "edge" | "firefox" | "chromium";

export type RestoreKind =
  | { type: "known_folder"; folder: string }
  | { type: "browser_profile"; browser: BrowserKind; profile_dir: string }
  | { type: "inventory"; name: string }
  | { type: Exclude<string, "known_folder" | "browser_profile" | "inventory"> };

export interface DiscoveryItem {
  id: string;
  category: Category;
  display_name: string;
  description: string;
  source: SourceRef;
  owner?: UserRef;
  estimated_size: number | null;
  item_count: number | null;
  access: AccessState;
  support: SupportLevel;
  selected_by_default: boolean;
  sensitive: boolean;
  opt_in_only: boolean;
  requires_admin: boolean;
  warnings: Warning[];
  restore_notes: string[];
  includes: string[];
  excludes: string[];
  restore_kind: RestoreKind;
}

export interface DriveInfo {
  mount_point: string;
  label: string;
  file_system: string;
  kind: "fixed" | "removable" | "network" | "unknown";
  total_bytes: number;
  free_bytes: number;
}

export interface MachineInfo {
  computer_name: string;
  os_name: string;
  os_version: string;
  os_build: string;
  architecture: string;
  cpu_summary: string;
  logical_cpus: number;
  total_memory_bytes: number;
  time_zone: string;
  join_state: "workgroup" | "domain" | "azure_ad" | "unknown";
  join_name: string | null;
  drives: DriveInfo[];
  network_adapters: { name: string; mac_address: string }[];
}

export interface UserProfile {
  sid: string;
  account_name: string;
  display_name: string | null;
  profile_path: string;
  profile_exists: boolean;
  is_current_user: boolean;
  is_system_account: boolean;
  last_use: string | null;
  last_use_confidence: "high" | "medium" | "low";
  size_bytes: number | null;
  access: AccessState;
}

export interface PrinterInfo {
  name: string;
  share_name: string | null;
  port_name: string;
  port_type: string | null;
  host_address: string | null;
  unc_path: string | null;
  connection: "usb" | "local" | "network" | "shared" | "wsd" | "virtual" | "other";
  driver_name: string;
  driver_version: string | null;
  is_default: boolean;
  status: string;
}

export interface MappedDrive {
  letter: string;
  unc_path: string;
  provider: string | null;
  persistent: boolean;
  status: string;
  label: string | null;
}

export interface InstalledApp {
  display_name: string;
  version: string | null;
  publisher: string | null;
  install_location: string | null;
  install_date: string | null;
  uninstall_command: string | null;
  architecture: string;
  scope: string;
  category: string;
  description: string | null;
  settings_plugin: string | null;
}

export interface BrowserProfileInfo {
  browser: BrowserKind;
  owner: UserRef;
  profile_dir: string;
  profile_name: string | null;
  path: string;
  size_bytes: number | null;
}

export interface ScanResult {
  scan_id: string;
  started_at: string;
  finished_at: string;
  machine: MachineInfo;
  elevated: boolean;
  users: UserProfile[];
  items: DiscoveryItem[];
  printers: PrinterInfo[];
  mapped_drives: MappedDrive[];
  applications: InstalledApp[];
  browser_profiles: BrowserProfileInfo[];
  running_processes: { name: string; pid: number }[];
  warnings: Warning[];
  platform: string;
}

export interface ScanProgress {
  stage: string;
  stage_index: number;
  stage_count: number;
  message: string;
}

export interface DiskSpace {
  path: string;
  total_bytes: number;
  free_bytes: number;
}

export interface PortablePaths {
  exe_path: string;
  exe_dir: string;
  exe_dir_writable: boolean;
  data_dir: string | null;
  default_destination: string | null;
  config_file: string;
  config_loaded: boolean;
}

export interface AppStatus {
  app_version: string;
  platform: string;
  fixture_mode: boolean;
  elevated: boolean;
  computer_name: string;
  paths: PortablePaths;
  authorization_acknowledged: boolean;
  session_destination: string | null;
  network_access: string;
}

export interface CaptureOptions {
  skip_locked_files: boolean;
  include_browser_cache: boolean;
  verify_after_copy: boolean;
  max_retries: number;
}

export interface CaptureRequest {
  destination_root: string;
  selected_item_ids: string[];
  encryption: { enabled: boolean; passphrase?: string };
  options: CaptureOptions;
}

export type TaskState =
  | "queued"
  | "scanning"
  | "copying"
  | "verifying"
  | "completed"
  | "completed_with_warnings"
  | "skipped"
  | "failed"
  | "canceled";

export interface TaskProgress {
  task_id: string;
  category: Category;
  display_name: string;
  state: TaskState;
  current_path: string | null;
  bytes_done: number;
  bytes_total: number | null;
  items_done: number;
  items_total: number | null;
  bytes_per_second: number | null;
  elapsed_ms: number;
  eta_seconds: number | null;
  warning_count: number;
  retry_count: number;
  error: string | null;
}

export interface LogEntry {
  timestamp: string;
  level: Severity;
  task_id: string | null;
  message: string;
}

export interface PreflightReport {
  destination_root: string;
  destination_writable: boolean;
  destination_free_bytes: number | null;
  source_free_bytes: number | null;
  estimated_bytes: number;
  unknown_size_items: number;
  sufficient_space: boolean;
  long_paths_enabled: boolean | null;
  destination_file_system: string | null;
  selected_count: number;
  running_apps: string[];
  warnings: Warning[];
  blocking_errors: string[];
}

export type BundleStatus = "in_progress" | "canceled" | "completed_unverified" | "verified" | "verified_with_warnings";

export interface CaptureSummary {
  bundle_id: string;
  bundle_path: string;
  machine_name: string;
  captured_users: string[];
  total_bytes: number;
  total_files: number;
  verified: boolean;
  status: BundleStatus;
  warnings: Warning[];
  task_results: TaskProgress[];
  report_html: string;
  report_json: string;
  summary_report_html: string;
}

export type CaptureStatus = "pending" | "captured" | "captured_with_warnings" | "skipped" | "failed" | "canceled";

export interface ManifestItem {
  id: string;
  category: Category;
  display_name: string;
  source_path: string;
  owner: UserRef | null;
  profile_relative: string | null;
  bundle_path: string;
  restore_kind: RestoreKind;
  support: SupportLevel;
  estimated_size: number | null;
  captured_bytes: number;
  captured_files: number;
  skipped_files: number;
  hash_status: "not_hashed" | "hashed" | "verified" | "mismatch";
  hash_list: string | null;
  hash_list_sha256: string | null;
  capture_status: CaptureStatus;
  warnings: Warning[];
  restore_notes: string[];
}

export interface ManifestUser {
  sid: string;
  account_name: string;
  display_name: string | null;
  profile_path: string;
  last_use: string | null;
  profile_size: number | null;
  bundle_dir: string;
  selected_modules: string[];
}

export interface RestoreEvent {
  restore_id: string;
  started_at: string;
  finished_at: string;
  target_computer: string;
  app_version: string;
  user_mappings: [string, string][];
  restored_items: string[];
  files_written: number;
  files_skipped: number;
  failures: number;
  outcome: string;
  report_path: string | null;
}

export interface Manifest {
  schema_version: string;
  bundle_id: string;
  app_version: string;
  created_at: string;
  completed_at: string | null;
  status: BundleStatus;
  source_machine: {
    computer_name: string;
    os_name: string;
    os_version: string;
    os_build: string;
    architecture: string;
    time_zone: string;
    join_state: string;
    join_name: string | null;
    device_id: string | null;
  };
  destination_root: string;
  elevated: boolean;
  encryption: { enabled: boolean; algorithm: string | null; kdf: unknown; key_check: string | null; chunk_size: number | null };
  users: ManifestUser[];
  selected_modules: string[];
  items: ManifestItem[];
  printers: PrinterInfo[];
  mapped_drives: MappedDrive[];
  applications: InstalledApp[];
  browser_profiles: BrowserProfileInfo[];
  restore_compatibility_notes: string[];
  integrity: {
    strategy: string;
    hash_algorithm: string;
    verified: boolean;
    verified_at: string | null;
    total_files: number;
    total_bytes: number;
    mismatches: number;
    bundle_root_hash: string | null;
  };
  restore_history: RestoreEvent[];
}

export interface BundleValidation {
  bundle_path: string;
  bundle_id: string;
  schema_version: string;
  schema_supported: boolean;
  manifest_hash_ok: boolean;
  encrypted: boolean;
  files_checked: number;
  mismatches: string[];
  missing: string[];
  errors: string[];
  ok: boolean;
}

export interface TargetInfo {
  computer_name: string;
  os_name: string;
  os_version: string;
  elevated: boolean;
  profiles: UserProfile[];
  running_processes: string[];
  free_bytes_system_drive: number | null;
}

export interface UserMapping {
  source_sid: string;
  target_sid: string | null;
}

export interface BundleOverview {
  validation: BundleValidation;
  manifest: Manifest;
  target: TargetInfo;
  suggested_mappings: UserMapping[];
}

export type CollisionPolicy = "skip_existing" | "rename_incoming" | "replace_after_confirmation";

export type SystemChange =
  | { type: "registry_value"; hive: string; key: string; value_name: string; value: string }
  | { type: "map_drive"; letter: string; unc_path: string; persistent: boolean }
  | { type: "connect_shared_printer"; unc_path: string }
  | { type: "add_network_printer"; name: string; host_address: string; driver_name: string; port_name: string }
  | { type: "manual_checklist"; text: string };

export interface RestoreAction {
  id: string;
  item_id: string;
  category: Category;
  display_name: string;
  source_user: string | null;
  target_user: string | null;
  target_path: string | null;
  files: number;
  bytes: number;
  conflicts: number;
  policy: CollisionPolicy;
  requires_admin: boolean;
  requires_confirmation: boolean;
  system_changes: SystemChange[];
  blocked_reason: string | null;
  warnings: Warning[];
  notes: string[];
}

export interface RestorePlan {
  plan_id: string;
  bundle_id: string;
  dry_run: boolean;
  actions: RestoreAction[];
  total_bytes: number;
  total_files: number;
  total_conflicts: number;
  target_free_bytes: number | null;
  warnings: Warning[];
}

export interface RestoreRequest {
  bundle_path: string;
  mappings: UserMapping[];
  selected_item_ids: string[];
  policies: Partial<Record<Category, CollisionPolicy>>;
  replace_confirmed: Category[];
  confirmed_categories: Category[];
  passphrase?: string;
}

export interface RestoreSummary {
  restore_id: string;
  bundle_id: string;
  files_written: number;
  files_skipped: number;
  files_renamed: number;
  files_replaced: number;
  failures: number;
  verified: boolean;
  outcome: string;
  task_results: TaskProgress[];
  warnings: Warning[];
  report_html: string;
}

export interface IndexedBundle {
  bundle_id: string;
  path: string;
  computer_name: string;
  created_at: string;
  status: string;
  last_event: string;
  updated_at: string;
}

export interface ReportView {
  kind: string;
  manifest: Manifest | null;
  html_path: string | null;
  summary_html_path: string | null;
  bundle_path: string | null;
}
