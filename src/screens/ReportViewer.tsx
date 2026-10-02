import { FileJson, FileText, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { CategoryIcon } from "../components/domain";
import { Alert, Badge, Button, Card, KeyValue } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes, formatDate } from "../lib/format";
import { CATEGORY_LABELS } from "../lib/labels";
import type { ReportView } from "../lib/types";

export function ReportViewer({ initialPath }: { initialPath?: string }) {
  const { backend, notify } = useApp();
  const [view, setView] = useState<ReportView | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function load(p: string) {
    setError(null);
    try {
      setView(await backend.loadReport(p));
    } catch (e) {
      setError(errorMessage(e));
    }
  }
  useEffect(() => {
    if (initialPath) load(initialPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialPath]);

  const m = view?.manifest;
  const open = (p: string | null) => p && backend.openPath(p).catch((e) => notify(errorMessage(e), "danger"));

  return (
    <div className="mx-auto max-w-5xl space-y-4 p-6">
      <Card className="space-y-3 p-5">
        <h2 className="text-lg font-semibold">Open an existing report</h2>
        <p className="text-[13px] text-muted">Choose a bundle folder, its manifest.json or report.json, or a report.html file.</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="primary" onClick={async () => { const p = await backend.pickFolder("Choose the bundle folder"); if (p) load(p); }}>
            <FolderOpen className="h-4 w-4" /> Bundle folder…
          </Button>
          <Button variant="outline" onClick={async () => { const p = await backend.pickFile("Choose a report or manifest", ["json", "html"]); if (p) load(p); }}>
            <FileJson className="h-4 w-4" /> Report or manifest file…
          </Button>
        </div>
        {error && <Alert tone="danger" title="Could not open">{error}</Alert>}
      </Card>
      {view && (
        <Card className="space-y-4 p-5">
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" disabled={!view.html_path} onClick={() => open(view.html_path)}><FileText className="h-4 w-4" /> Technician report</Button>
            <Button variant="outline" disabled={!view.summary_html_path} onClick={() => open(view.summary_html_path)}>Shareable summary</Button>
            <Button variant="ghost" disabled={!view.bundle_path} onClick={() => open(view.bundle_path)}><FolderOpen className="h-4 w-4" /> Bundle folder</Button>
          </div>
          {m && (
            <>
              <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                <KeyValue label="Bundle ID" value={m.bundle_id} mono />
                <KeyValue label="Computer" value={m.source_machine.computer_name} />
                <KeyValue label="Captured" value={formatDate(m.created_at)} />
                <KeyValue label="App version" value={m.app_version} />
                <KeyValue label="Status" value={<Badge tone={m.integrity.verified ? "ok" : "danger"}>{m.status.replace(/_/g, " ")}</Badge>} />
                <KeyValue label="Size" value={`${formatBytes(m.integrity.total_bytes)} · ${m.integrity.total_files} files`} />
                <KeyValue label="Encryption" value={m.encryption.enabled ? m.encryption.algorithm : "Off"} />
                <KeyValue label="Restores" value={m.restore_history.length} />
              </dl>
              <table className="w-full text-[13px]">
                <thead className="text-left text-[12px] text-muted">
                  <tr><th className="py-1 font-medium">Item</th><th className="font-medium">Owner</th><th className="font-medium">Files</th><th className="font-medium">Size</th><th className="font-medium">Status</th></tr>
                </thead>
                <tbody>
                  {m.items.map((i) => (
                    <tr key={i.id} className="border-t border-border">
                      <td className="py-1.5"><span className="flex items-center gap-2"><CategoryIcon category={i.category} className="text-muted" />{i.display_name}</span><span className="sr-only">{CATEGORY_LABELS[i.category]}</span></td>
                      <td>{i.owner?.account_name ?? "Computer"}</td>
                      <td>{i.captured_files}</td>
                      <td>{formatBytes(i.captured_bytes)}</td>
                      <td>{i.capture_status.replace(/_/g, " ")}{i.warnings.length ? <Badge tone="warn" className="ml-1">{i.warnings.length}</Badge> : null}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </Card>
      )}
    </div>
  );
}
