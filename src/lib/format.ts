// Formatting helpers. Sizes use 1024 multiples with Windows-style unit names
// (matching src-tauri/src/util.rs so UI and reports agree).

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return "Unknown";
  if (bytes < 1024) return bytes === 1 ? "1 byte" : `${Math.round(bytes)} bytes`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[i]}`;
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return "—";
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const r = s % 60;
  if (h > 0) return `${h} h ${String(m).padStart(2, "0")} min`;
  if (m > 0) return `${m} min ${String(r).padStart(2, "0")} s`;
  return `${r} s`;
}

/** Remaining-time label that never pretends to be exact. */
export function formatEta(eta: number | null | undefined, terminal: boolean): string {
  if (terminal) return "—";
  if (eta === null || eta === undefined) return "Estimating…";
  if (eta === 0) return "Finishing…";
  return `About ${formatDuration(eta)} left`;
}

export function formatRate(bps: number | null | undefined): string {
  if (!bps || bps <= 0) return "—";
  return `${formatBytes(bps)}/s`;
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "Unknown";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "Unknown";
  return d.toLocaleString(undefined, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** Truncate a long path in the middle, keeping the drive and file name. */
export function truncateMiddle(text: string, max = 64): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.4);
  const tail = keep - head;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}

export function percent(done: number, total: number | null | undefined): number | null {
  if (!total || total <= 0) return null;
  return Math.max(0, Math.min(100, (done / total) * 100));
}
