// Selection rules shared by the dashboard and tests.

import type { DiscoveryItem, ScanResult } from "./types";

/** Items that can be ticked at all. */
export function isSelectable(item: DiscoveryItem, allowSensitive: boolean): boolean {
  if (item.support === "unsupported") return false;
  if (item.access === "access_denied" || item.access === "not_found") return false;
  if (item.sensitive && !allowSensitive) return false;
  return true;
}

/**
 * "Select all safe items": capturable, readable, not privacy-sensitive, not
 * opt-in-only and without error-level warnings. Items of other users are
 * included only when `includeOtherUsers` is set by the technician.
 */
export function isSafe(item: DiscoveryItem, scan: ScanResult, includeOtherUsers: boolean): boolean {
  if (!isSelectable(item, false) || item.opt_in_only || item.sensitive) return false;
  if (item.warnings.some((w) => w.severity === "error")) return false;
  if (item.owner && !includeOtherUsers) {
    const u = scan.users.find((x) => x.sid === item.owner!.sid);
    if (!u?.is_current_user) return false;
  }
  if (item.display_name === "Microsoft Print to PDF") return false;
  return true;
}

export function defaultSelection(scan: ScanResult): Set<string> {
  return new Set(scan.items.filter((i) => i.selected_by_default).map((i) => i.id));
}

export function selectAllSafe(scan: ScanResult, current: Set<string>, includeOtherUsers: boolean): Set<string> {
  const next = new Set(current);
  scan.items.filter((i) => isSafe(i, scan, includeOtherUsers)).forEach((i) => next.add(i.id));
  return next;
}

export function matchesFilter(item: DiscoveryItem, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = [
    item.display_name,
    item.description,
    item.owner?.account_name ?? "",
    item.source.kind === "path" ? item.source.path : item.source.reference,
  ]
    .join(" ")
    .toLowerCase();
  return q.split(/\s+/).every((t) => hay.includes(t));
}

export function selectionTotals(scan: ScanResult, selected: Set<string>) {
  const items = scan.items.filter((i) => selected.has(i.id));
  return {
    count: items.length,
    bytes: items.reduce((a, i) => a + (i.estimated_size ?? 0), 0),
    // Only file-backed items can have an unknown size; inventories are tiny JSON files.
    unknownSize: items.filter((i) => i.estimated_size === null && i.source.kind === "path" && i.support !== "inventory_only").length,
    files: items.reduce((a, i) => a + (i.category === "users_files" || i.category === "desktop_shortcuts" ? i.item_count ?? 0 : 0), 0),
    warnings: items.reduce((a, i) => a + i.warnings.filter((w) => w.severity !== "info").length, 0),
    users: new Set(items.filter((i) => i.owner).map((i) => i.owner!.sid)).size,
  };
}
