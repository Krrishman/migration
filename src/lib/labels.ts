import type { AccessState, Category, SupportLevel, TaskState, CollisionPolicy } from "./types";

export const CATEGORY_LABELS: Record<Category, string> = {
  users_files: "Users & Files",
  browsers: "Browsers",
  outlook_email: "Outlook & Email",
  personalization: "Personalization",
  printers: "Printers",
  network_drives: "Network Drives",
  desktop_shortcuts: "Desktop & Shortcuts",
  application_settings: "Application Settings",
  installed_applications: "Installed Applications",
  system_inventory: "System Inventory",
};

export const CATEGORY_ORDER: Category[] = [
  "users_files",
  "browsers",
  "outlook_email",
  "personalization",
  "printers",
  "network_drives",
  "desktop_shortcuts",
  "application_settings",
  "installed_applications",
  "system_inventory",
];

export const SUPPORT_LABELS: Record<SupportLevel, string> = {
  supported: "Supported",
  partial: "Partial",
  inventory_only: "Inventory only",
  unsupported: "Unsupported",
};

export const ACCESS_LABELS: Record<AccessState, string> = {
  accessible: "Accessible",
  partially_accessible: "Partly accessible",
  access_denied: "Access denied",
  locked: "Locked",
  not_found: "Not found",
  unknown: "Unknown",
};

export const TASK_STATE_LABELS: Record<TaskState, string> = {
  queued: "Queued",
  scanning: "Scanning",
  copying: "Copying",
  verifying: "Verifying",
  completed: "Completed",
  completed_with_warnings: "Completed with warnings",
  skipped: "Skipped",
  failed: "Failed",
  canceled: "Canceled",
};

export const POLICY_LABELS: Record<CollisionPolicy, string> = {
  skip_existing: "Skip existing",
  rename_incoming: "Rename incoming",
  replace_after_confirmation: "Replace after confirmation",
};

export const TERMINAL_STATES: TaskState[] = ["completed", "completed_with_warnings", "skipped", "failed", "canceled"];
