import { useEffect, useMemo, useRef, useState } from "react";
import { LogDrawer, TaskCard } from "../components/domain";
import { Alert, Button, Card, Dialog, Progress } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes, formatDuration } from "../lib/format";
import { TERMINAL_STATES } from "../lib/labels";
import type { CaptureRequest, CaptureSummary, LogEntry, TaskProgress } from "../lib/types";

/** Overall progress derived truthfully from task snapshots. */
export function overall(tasks: TaskProgress[]) {
  const done = tasks.filter((t) => TERMINAL_STATES.includes(t.state)).length;
  const bytesTotal = tasks.reduce((a, t) => a + (t.bytes_total ?? 0), 0);
  const bytesDone = tasks.reduce((a, t) => a + (TERMINAL_STATES.includes(t.state) ? (t.bytes_total ?? t.bytes_done) : t.bytes_done), 0);
  const unknownTotals = tasks.some((t) => !TERMINAL_STATES.includes(t.state) && t.bytes_total === null && t.state !== "queued");
  const pct = tasks.length === 0 ? 0 : bytesTotal > 0 && !unknownTotals ? (bytesDone / bytesTotal) * 100 : (done / tasks.length) * 100;
  return { done, total: tasks.length, bytesDone, bytesTotal, pct: Math.min(100, pct) };
}

export function CaptureScreen({
  request,
  resume,
  onDone,
}: {
  request: CaptureRequest;
  resume?: { bundlePath: string; passphrase: string | null };
  onDone: (s: CaptureSummary) => void;
}) {
  const { backend } = useApp();
  const [tasks, setTasks] = useState<Map<string, TaskProgress>>(new Map());
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [canceling, setCanceling] = useState(false);
  const started = useRef(Date.now());
  const [now, setNow] = useState(Date.now());
  const ran = useRef(false);

  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    if (ran.current) return; // StrictMode double-invoke guard: never start twice
    ran.current = true;
    let unP: (() => void) | undefined;
    let unL: (() => void) | undefined;
    (async () => {
      unP = await backend.on<TaskProgress>("capture://progress", (p) =>
        setTasks((m) => {
          const n = new Map(m);
          n.set(p.task_id, p);
          return n;
        }),
      );
      unL = await backend.on<LogEntry>("capture://log", (l) => setLogs((x) => [...x, l]));
      try {
        const s = resume ? await backend.resumeCapture(resume.bundlePath, resume.passphrase) : await backend.startCapture(request);
        onDone(s);
      } catch (e) {
        setError(errorMessage(e));
      } finally {
        unP?.();
        unL?.();
      }
    })();
  }, [backend, request, resume, onDone]);

  const list = useMemo(() => [...tasks.values()], [tasks]);
  const o = overall(list);

  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <Card className="p-5">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold">{resume ? "Resuming capture" : "Capturing selected items"}</h2>
            <p className="text-[13px] text-muted">
              {o.done} of {o.total} tasks finished · {formatBytes(o.bytesDone)}
              {o.bytesTotal ? ` of ${formatBytes(o.bytesTotal)}` : ""} · elapsed {formatDuration((now - started.current) / 1000)}
            </p>
          </div>
          <Button variant="outline" onClick={() => setConfirmCancel(true)} disabled={canceling || !!error}>
            {canceling ? "Canceling…" : "Cancel"}
          </Button>
        </div>
        <Progress className="mt-4 h-3" value={o.pct} label="Overall capture progress" />
        <p className="mt-2 text-[12px] text-muted">Source files are only read. Each file is written to a temporary name, verified with SHA-256 and then renamed into place.</p>
      </Card>
      {error && (
        <Alert tone="danger" title="Capture stopped">
          {error}
        </Alert>
      )}
      <div className="grid gap-3 md:grid-cols-2" aria-live="off">
        {list.map((t) => (
          <TaskCard key={t.task_id} task={t} />
        ))}
      </div>
      <LogDrawer logs={logs} />
      <Dialog
        open={confirmCancel}
        onOpenChange={setConfirmCancel}
        title="Cancel the capture?"
        description="Files already copied and verified are kept. You can resume this bundle later from the completion screen."
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmCancel(false)}>
              Keep capturing
            </Button>
            <Button
              variant="danger"
              onClick={async () => {
                setConfirmCancel(false);
                setCanceling(true);
                await backend.cancelCapture();
              }}
            >
              Cancel capture
            </Button>
          </>
        }
      />
    </div>
  );
}
