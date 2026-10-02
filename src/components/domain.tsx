// Domain-specific presentation components shared by several screens.

import clsx from "clsx";
import {
  AppWindow,
  Cpu,
  FileText,
  FolderOpen,
  Globe,
  HardDrive,
  Image as ImageIcon,
  Mail,
  Monitor,
  Network,
  Printer,
  Settings2,
  Users,
} from "lucide-react";
import { useMemo, useState } from "react";
import { formatBytes, formatDuration, formatEta, formatRate, percent, truncateMiddle } from "../lib/format";
import { CATEGORY_LABELS, SUPPORT_LABELS, TASK_STATE_LABELS, TERMINAL_STATES } from "../lib/labels";
import type { Category, DiscoveryItem, LogEntry, Severity, TaskProgress } from "../lib/types";
import { Badge, Button, CopyButton, Progress, type Tone } from "./ui";

const ICONS: Record<Category, typeof Users> = {
  users_files: Users,
  browsers: Globe,
  outlook_email: Mail,
  personalization: ImageIcon,
  printers: Printer,
  network_drives: Network,
  desktop_shortcuts: Monitor,
  application_settings: Settings2,
  installed_applications: AppWindow,
  system_inventory: Cpu,
};

export function CategoryIcon({ category, className }: { category: Category; className?: string }) {
  const Icon = ICONS[category] ?? FolderOpen;
  return <Icon className={clsx("h-4 w-4", className)} aria-hidden />;
}

export function supportTone(s: DiscoveryItem["support"]): Tone {
  return s === "supported" ? "ok" : s === "partial" ? "teal" : s === "inventory_only" ? "accent" : "neutral";
}

/** Status chips required by the dashboard: support level, admin, locked, excluded, warnings. */
export function StatusChips({ item }: { item: DiscoveryItem }) {
  const warn = item.warnings.filter((w) => w.severity !== "info").length;
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Badge tone={supportTone(item.support)}>{SUPPORT_LABELS[item.support]}</Badge>
      {item.requires_admin && <Badge tone="warn">Admin required</Badge>}
      {item.access === "locked" && <Badge tone="warn">Locked</Badge>}
      {item.access === "access_denied" && <Badge tone="danger">Access denied</Badge>}
      {item.support === "unsupported" && <Badge tone="neutral">Excluded</Badge>}
      {item.sensitive && <Badge tone="warn">Privacy-sensitive</Badge>}
      {item.opt_in_only && !item.sensitive && <Badge tone="neutral">Opt-in</Badge>}
      {warn > 0 && (
        <Badge tone="warn" title={item.warnings.map((w) => w.message).join("\n")}>
          {warn} warning{warn === 1 ? "" : "s"}
        </Badge>
      )}
    </div>
  );
}

function stateTone(s: TaskProgress["state"]): Tone {
  switch (s) {
    case "completed":
      return "ok";
    case "completed_with_warnings":
      return "warn";
    case "failed":
      return "danger";
    case "skipped":
    case "canceled":
    case "queued":
      return "neutral";
    default:
      return "accent";
  }
}

export function TaskCard({ task }: { task: TaskProgress }) {
  const terminal = TERMINAL_STATES.includes(task.state);
  const pct = task.state === "completed" || task.state === "completed_with_warnings" ? 100 : percent(task.bytes_done, task.bytes_total) ?? (task.items_total ? percent(task.items_done, task.items_total) : null);
  const tone = task.state === "failed" ? "danger" : task.state === "completed_with_warnings" ? "warn" : task.state === "completed" ? "ok" : "accent";
  return (
    <div className="rounded-lg border border-border bg-surface p-3" data-testid="task-card" data-state={task.state}>
      <div className="flex items-center gap-2">
        <span className="flex h-7 w-7 items-center justify-center rounded-md bg-surface-2 text-muted">
          <CategoryIcon category={task.category} />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium" title={task.display_name}>
            {task.display_name}
          </p>
          <p className="text-[12px] text-muted">{CATEGORY_LABELS[task.category]}</p>
        </div>
        <Badge tone={stateTone(task.state)}>{TASK_STATE_LABELS[task.state]}</Badge>
      </div>
      <Progress className="mt-3" value={task.state === "queued" ? 0 : pct === null && !terminal ? null : pct ?? 0} tone={tone} label={`${task.display_name} progress`} />
      <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1 text-[12px] text-muted sm:grid-cols-4">
        <span>
          {formatBytes(task.bytes_done)}
          {task.bytes_total ? ` of ${formatBytes(task.bytes_total)}` : ""}
        </span>
        <span>
          {task.items_done.toLocaleString()}
          {task.items_total !== null ? ` / ${task.items_total.toLocaleString()}` : ""} items
        </span>
        <span>{terminal ? (task.state === "skipped" || task.state === "canceled" ? "—" : `Took ${formatDuration(task.elapsed_ms / 1000)}`) : formatRate(task.bytes_per_second)}</span>
        <span>{task.state === "queued" ? "Waiting" : formatEta(task.eta_seconds, terminal)}</span>
      </div>
      {task.current_path && !terminal && (
        <div className="mt-2 flex items-center gap-1 rounded bg-surface-2 px-2 py-1 font-mono text-[11px] text-muted">
          <span className="min-w-0 flex-1 truncate" title={task.current_path}>
            {truncateMiddle(task.current_path, 90)}
          </span>
          <CopyButton text={task.current_path} />
        </div>
      )}
      {(task.warning_count > 0 || task.retry_count > 0 || task.error) && (
        <div className="mt-2 flex flex-wrap gap-2 text-[12px]">
          {task.warning_count > 0 && <Badge tone="warn">{task.warning_count} warning(s)</Badge>}
          {task.retry_count > 0 && <Badge tone="neutral">{task.retry_count} retr{task.retry_count === 1 ? "y" : "ies"}</Badge>}
          {task.error && <span className="text-danger">{task.error}</span>}
        </div>
      )}
    </div>
  );
}

export function LogDrawer({ logs }: { logs: LogEntry[] }) {
  const [filter, setFilter] = useState<Severity | "all">("all");
  const shown = useMemo(() => logs.filter((l) => filter === "all" || l.level === filter).slice(-400), [logs, filter]);
  const counts = useMemo(() => ({ info: logs.filter((l) => l.level === "info").length, warning: logs.filter((l) => l.level === "warning").length, error: logs.filter((l) => l.level === "error").length }), [logs]);
  return (
    <details className="rounded-xl border border-border bg-surface">
      <summary className="flex cursor-pointer items-center gap-2 px-4 py-3 font-medium">
        <FileText className="h-4 w-4 text-muted" aria-hidden /> Activity log
        <span className="ml-auto flex gap-1 text-[12px]">
          <Badge>{counts.info} info</Badge>
          <Badge tone="warn">{counts.warning} warnings</Badge>
          <Badge tone="danger">{counts.error} errors</Badge>
        </span>
      </summary>
      <div className="border-t border-border px-4 py-3">
        <div className="mb-2 flex gap-1" role="group" aria-label="Filter log">
          {(["all", "info", "warning", "error"] as const).map((f) => (
            <Button key={f} size="sm" variant={filter === f ? "primary" : "ghost"} onClick={() => setFilter(f)} aria-pressed={filter === f}>
              {f === "all" ? "All" : f[0].toUpperCase() + f.slice(1)}
            </Button>
          ))}
        </div>
        <ol className="scroll-thin max-h-56 space-y-1 overflow-y-auto font-mono text-[12px]" aria-live="polite">
          {shown.length === 0 && <li className="text-muted">No entries.</li>}
          {shown.map((l, i) => (
            <li key={i} className={clsx(l.level === "error" && "text-danger", l.level === "warning" && "text-warn")}>
              <span className="text-muted">{new Date(l.timestamp).toLocaleTimeString()}</span> {l.message}
            </li>
          ))}
        </ol>
      </div>
    </details>
  );
}

export function FreeSpace({ label, free, total, needed }: { label: string; free: number | null | undefined; total?: number | null; needed?: number }) {
  const used = total && free !== null && free !== undefined ? ((total - free) / total) * 100 : null;
  const insufficient = needed !== undefined && free !== null && free !== undefined && free < needed;
  return (
    <div className="min-w-0">
      <div className="flex items-center gap-1 text-[12px] text-muted">
        <HardDrive className="h-3.5 w-3.5" aria-hidden /> {label}
      </div>
      <p className={clsx("font-medium", insufficient && "text-danger")}>{free === null || free === undefined ? "Unknown" : `${formatBytes(free)} free`}</p>
      {used !== null && <Progress className="mt-1 h-1.5" value={used} tone={insufficient ? "danger" : "accent"} label={`${label} used space`} />}
    </div>
  );
}
