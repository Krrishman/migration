import { CheckCircle2, FileText, FolderOpen, ListChecks, RotateCcw, ShieldAlert, Trash2, XCircle } from "lucide-react";
import { useState } from "react";
import { TaskCard } from "../components/domain";
import { Alert, Badge, Button, Card, Dialog, Input, KeyValue } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes } from "../lib/format";
import type { CaptureSummary } from "../lib/types";

export function CompletionScreen({
  summary,
  onAnotherScan,
  onResume,
  onHome,
}: {
  summary: CaptureSummary;
  onAnotherScan: () => void;
  onResume: (bundlePath: string, passphrase: string | null) => void;
  onHome: () => void;
}) {
  const { backend, notify } = useApp();
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [confirmId, setConfirmId] = useState("");
  const [resumePass, setResumePass] = useState("");
  const incomplete = summary.status === "canceled" || summary.status === "in_progress" || summary.status === "completed_unverified";
  const open = (p: string) => backend.openPath(p).catch((e) => notify(errorMessage(e), "danger"));

  const headline =
    summary.status === "verified"
      ? { icon: <CheckCircle2 className="h-8 w-8 text-ok" />, text: "Backup complete and verified" }
      : summary.status === "verified_with_warnings"
        ? { icon: <ShieldAlert className="h-8 w-8 text-warn" />, text: "Backup verified, with warnings" }
        : summary.status === "canceled"
          ? { icon: <XCircle className="h-8 w-8 text-muted" />, text: "Backup canceled — incomplete" }
          : { icon: <XCircle className="h-8 w-8 text-danger" />, text: "Backup finished but is NOT verified" };

  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <Card className="p-5">
        <div className="flex items-center gap-3">
          {headline.icon}
          <div>
            <h2 className="text-xl font-semibold">{headline.text}</h2>
            <p className="text-[13px] text-muted">Status is derived from task outcomes and a final integrity check of every hash list.</p>
          </div>
        </div>
        <dl className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <KeyValue label="Bundle folder" value={<span title={summary.bundle_path}>{summary.bundle_path}</span>} mono />
          <KeyValue label="Bundle ID" value={summary.bundle_id} mono />
          <KeyValue label="Computer" value={summary.machine_name} />
          <KeyValue label="Users" value={summary.captured_users.join(", ") || "—"} />
          <KeyValue label="Total size" value={formatBytes(summary.total_bytes)} />
          <KeyValue label="Files" value={summary.total_files.toLocaleString()} />
          <KeyValue label="Verification" value={summary.verified ? <Badge tone="ok">SHA-256 verified</Badge> : <Badge tone="danger">Not verified</Badge>} />
          <KeyValue label="Warnings" value={summary.warnings.length} />
        </dl>
        <div className="mt-5 flex flex-wrap gap-2">
          <Button variant="primary" onClick={() => open(summary.bundle_path)}>
            <FolderOpen className="h-4 w-4" /> Open bundle folder
          </Button>
          <Button variant="outline" onClick={() => open(summary.report_html)}>
            <FileText className="h-4 w-4" /> Technician report (HTML)
          </Button>
          <Button variant="outline" onClick={() => open(summary.report_json)}>
            JSON report
          </Button>
          <Button variant="outline" onClick={() => open(summary.summary_report_html)}>
            Shareable summary (redacted)
          </Button>
          {!incomplete && (
            <Button
              variant="outline"
              onClick={() =>
                backend.prepareRestoreInstructions(summary.bundle_path).then(
                  (p) => notify(`Restore instructions saved to ${p}`, "ok"),
                  (e) => notify(errorMessage(e), "danger"),
                )
              }
            >
              <ListChecks className="h-4 w-4" /> Prepare restore instructions
            </Button>
          )}
          <Button variant="ghost" onClick={onAnotherScan}>
            <RotateCcw className="h-4 w-4" /> Start another scan
          </Button>
        </div>
      </Card>

      {incomplete && (
        <Alert tone="warn" title="This bundle is incomplete">
          <p>Completed files are kept and checkpointed. Resume to finish the remaining tasks, or delete the incomplete bundle.</p>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <Input type="password" className="w-64" placeholder="Passphrase (only if encrypted)" aria-label="Passphrase to resume" value={resumePass} onChange={(e) => setResumePass(e.target.value)} />
            <Button variant="primary" size="sm" onClick={() => onResume(summary.bundle_path, resumePass || null)}>
              Resume capture
            </Button>
            <Button variant="danger" size="sm" onClick={() => setDeleteOpen(true)}>
              <Trash2 className="h-4 w-4" /> Delete incomplete bundle…
            </Button>
          </div>
        </Alert>
      )}

      {summary.warnings.length > 0 && (
        <Card className="p-5">
          <h3 className="mb-2 font-semibold">Warnings</h3>
          <ul className="space-y-1 text-[13px]">
            {summary.warnings.slice(0, 50).map((w, i) => (
              <li key={i} className={w.severity === "error" ? "text-danger" : "text-warn"}>
                {w.message} {w.path && <span className="font-mono text-[11px] text-muted">{w.path}</span>}
              </li>
            ))}
          </ul>
        </Card>
      )}

      <div className="grid gap-3 md:grid-cols-2">
        {summary.task_results.map((t) => (
          <TaskCard key={t.task_id} task={t} />
        ))}
      </div>
      <Button variant="ghost" onClick={onHome}>
        Back to home
      </Button>

      <Dialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title="Delete incomplete bundle?"
        description="This permanently deletes the bundle folder on the destination drive. Source data on this PC is not affected."
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleteOpen(false)}>
              Keep bundle
            </Button>
            <Button
              variant="danger"
              disabled={confirmId !== summary.bundle_id}
              onClick={async () => {
                try {
                  await backend.deleteIncompleteBundle(summary.bundle_path, confirmId);
                  setDeleteOpen(false);
                  notify("Incomplete bundle deleted.", "ok");
                  onHome();
                } catch (e) {
                  notify(errorMessage(e), "danger");
                }
              }}
            >
              Delete permanently
            </Button>
          </>
        }
      >
        <p className="font-mono text-[12px]">{summary.bundle_path}</p>
        <label className="block text-[13px]">
          Type the bundle ID to confirm: <span className="font-mono">{summary.bundle_id}</span>
          <Input className="mt-1 font-mono" value={confirmId} onChange={(e) => setConfirmId(e.target.value)} aria-label="Bundle ID confirmation" />
        </label>
      </Dialog>
    </div>
  );
}
