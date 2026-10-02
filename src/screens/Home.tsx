import { DatabaseBackup, FileSearch, FolderOpen, HardDriveDownload, ShieldCheck, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import type { View } from "../App";
import { Badge, Button, Card, Dialog, KeyValue, SectionTitle } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatDate } from "../lib/format";
import type { IndexedBundle } from "../lib/types";
import { PrivacyContent } from "./Authorization";

export function Home({ onNavigate }: { onNavigate: (v: View) => void }) {
  const { backend, status, notify } = useApp();
  const [bundles, setBundles] = useState<IndexedBundle[]>([]);
  const [privacy, setPrivacy] = useState(false);

  useEffect(() => {
    backend.listBundles().then(setBundles).catch(() => setBundles([]));
  }, [backend]);

  const actions = [
    { icon: HardDriveDownload, title: "Create migration backup", text: "Scan this PC, choose what to keep and write a verified bundle to a USB or external drive.", view: { name: "scan" } as View, primary: true },
    { icon: Upload, title: "Restore migration backup", text: "Open a bundle on the new PC, map users, review a dry-run plan and restore selected items.", view: { name: "restore" } as View },
    { icon: FileSearch, title: "Open existing report", text: "View the technician report, inventory and verification results of a bundle.", view: { name: "report" } as View },
  ];

  return (
    <div className="mx-auto max-w-6xl space-y-6 p-6">
      <div>
        <h2 className="text-2xl font-semibold">Welcome</h2>
        <p className="text-muted">Move user files and supported settings from one Windows PC to another — locally, with verification and clear reports.</p>
      </div>
      <div className="grid gap-4 md:grid-cols-3">
        {actions.map((a) => (
          <button
            key={a.title}
            onClick={() => onNavigate(a.view)}
            className={`group rounded-xl border p-5 text-left shadow-sm transition hover:-translate-y-0.5 hover:shadow-md focus-visible:outline-2 ${a.primary ? "border-accent/40 bg-accent/5" : "border-border bg-surface"}`}
          >
            <a.icon className={`h-7 w-7 ${a.primary ? "text-accent" : "text-teal"}`} aria-hidden />
            <p className="mt-3 text-[16px] font-semibold">{a.title}</p>
            <p className="mt-1 text-[13px] text-muted">{a.text}</p>
          </button>
        ))}
      </div>

      <div className="grid gap-4 lg:grid-cols-5">
        <Card className="p-5 lg:col-span-3">
          <SectionTitle actions={<Button size="sm" variant="ghost" onClick={() => setPrivacy(true)}>Privacy notice</Button>}>Portable mode</SectionTitle>
          <dl className="grid gap-4 sm:grid-cols-2">
            <KeyValue label="Executable" value={<span title={status.paths.exe_path}>{status.paths.exe_path}</span>} mono />
            <KeyValue label="Application data" value={status.paths.data_dir ?? "Not stored (folder is read-only)"} mono />
            <KeyValue label="Default backup destination" value={status.session_destination ?? "Choose a folder when creating a backup"} mono />
            <KeyValue
              label="Executable folder"
              value={status.paths.exe_dir_writable ? <Badge tone="ok">Writable — portable data is kept beside the app</Badge> : <Badge tone="warn">Read-only — choose a destination each session</Badge>}
            />
            <KeyValue label="Process" value={status.elevated ? <Badge tone="warn">Running as administrator</Badge> : <Badge>Standard user (recommended)</Badge>} />
            <KeyValue label="Network" value={<Badge tone="ok"><ShieldCheck className="h-3.5 w-3.5" /> No network access, no telemetry</Badge>} />
          </dl>
          <p className="mt-4 text-[12px] text-muted">
            Data source: <span className="font-mono">{status.platform}</span> · Version {status.app_version}. Nothing is written to AppData. Backups are stored only in the destination
            you choose.
          </p>
        </Card>
        <Card className="p-5 lg:col-span-2">
          <SectionTitle>Recent bundles</SectionTitle>
          {bundles.length === 0 ? (
            <p className="text-[13px] text-muted">Bundles created or restored with this copy of Migration Assistant appear here.</p>
          ) : (
            <ul className="space-y-2">
              {bundles.slice(0, 5).map((b) => (
                <li key={b.bundle_id} className="flex items-center gap-3 rounded-lg border border-border p-3">
                  <DatabaseBackup className="h-5 w-5 shrink-0 text-muted" aria-hidden />
                  <div className="min-w-0 flex-1">
                    <p className="truncate font-medium">{b.computer_name}</p>
                    <p className="truncate text-[12px] text-muted" title={b.path}>
                      {formatDate(b.created_at)} · {b.status} · last {b.last_event}
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Open folder of bundle from ${b.computer_name}`}
                    onClick={() => backend.openPath(b.path).catch((e) => notify(errorMessage(e), "danger"))}
                  >
                    <FolderOpen className="h-4 w-4" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </Card>
      </div>
      <Dialog open={privacy} onOpenChange={setPrivacy} title="Privacy notice" wide>
        <PrivacyContent />
      </Dialog>
    </div>
  );
}
