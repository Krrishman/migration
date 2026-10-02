import { CheckCircle2, Circle, FolderOpen, Loader2, RefreshCw, Save, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { FreeSpace } from "../components/domain";
import { Alert, Button, Card, Checkbox, Progress, SectionTitle } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes } from "../lib/format";
import { defaultSelection } from "../lib/selection";
import type { DiskSpace, ScanProgress, ScanResult } from "../lib/types";

export function SourceScan({ scan, onScanned, onContinue }: { scan: ScanResult | null; onScanned: (s: ScanResult, sel: Set<string>) => void; onContinue: () => void }) {
  const { backend, status, refreshStatus, notify } = useApp();
  const [authorized, setAuthorized] = useState(!!scan);
  const [destination, setDestination] = useState<string | null>(status.session_destination);
  const [destSpace, setDestSpace] = useState<DiskSpace | null>(null);
  const [srcSpace, setSrcSpace] = useState<DiskSpace | null>(null);
  const [scanning, setScanning] = useState(false);
  const [stages, setStages] = useState<ScanProgress[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (destination) backend.diskSpace(destination).then(setDestSpace).catch(() => setDestSpace(null));
  }, [backend, destination]);
  useEffect(() => {
    const p = scan?.users.find((u) => u.is_current_user)?.profile_path ?? "C:\\";
    backend.diskSpace(p).then(setSrcSpace).catch(() => setSrcSpace(null));
  }, [backend, scan]);

  async function chooseDestination() {
    const p = await backend.pickFolder("Choose where to save the migration bundle");
    if (!p) return;
    try {
      const d = await backend.setDestination(p);
      setDestination(d);
      await refreshStatus();
    } catch (e) {
      notify(errorMessage(e), "danger");
    }
  }

  async function runScan() {
    setScanning(true);
    setError(null);
    setStages([]);
    const un = await backend.on<ScanProgress>("scan://progress", (p) => setStages((s) => [...s.filter((x) => x.stage_index !== p.stage_index), p]));
    try {
      const r = await backend.startScan();
      onScanned(r, defaultSelection(r));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      un();
      setScanning(false);
    }
  }

  const last = stages[stages.length - 1];
  const pct = last ? ((last.stage_index + (scanning ? 0.5 : 1)) / last.stage_count) * 100 : scan ? 100 : 0;

  return (
    <div className="mx-auto max-w-4xl space-y-4 p-6">
      <Card className="p-5">
        <SectionTitle>1. Device authorization</SectionTitle>
        <label className="flex items-start gap-3">
          <Checkbox label="Authorized to scan this device" checked={authorized} onChange={(e) => setAuthorized(e.target.checked)} className="mt-1" />
          <span>
            I confirm that scanning <strong>{status.computer_name}</strong> and copying the data I select is authorized. The scan only reads this computer; nothing leaves it except
            the bundle you write to your chosen destination.
          </span>
        </label>
      </Card>

      <Card className="p-5">
        <SectionTitle
          actions={
            <>
              <Button size="sm" variant="outline" onClick={chooseDestination}>
                <FolderOpen className="h-4 w-4" /> Change…
              </Button>
              {status.paths.exe_dir_writable && destination && (
                <Button
                  size="sm"
                  variant="ghost"
                  title={`Saves ${status.paths.config_file}`}
                  onClick={() => backend.saveConfig().then((p) => notify(`Saved default destination to ${p}`, "ok"), (e) => notify(errorMessage(e), "danger"))}
                >
                  <Save className="h-4 w-4" /> Save as default
                </Button>
              )}
            </>
          }
        >
          2. Destination
        </SectionTitle>
        {destination ? (
          <p className="font-mono text-[13px]">
            {destination}
            <span className="text-muted">\migrations\&lt;computer&gt;-&lt;date&gt;-&lt;id&gt;\</span>
          </p>
        ) : (
          <Alert tone="warn" title="Choose a destination">
            The application folder is read-only, so there is no default. Choose a USB drive, external disk or a writable local folder. The choice is kept for this session only unless
            you save it.
          </Alert>
        )}
        <div className="mt-4 grid gap-4 sm:grid-cols-2">
          <FreeSpace label="Source (this PC's profile drive)" free={srcSpace?.free_bytes} total={srcSpace?.total_bytes} />
          <FreeSpace label="Destination" free={destSpace?.free_bytes} total={destSpace?.total_bytes} />
        </div>
      </Card>

      {!status.elevated && (
        <Alert
          tone="accent"
          icon={<ShieldAlert className="h-5 w-5 text-accent" />}
          title="Running as a standard user"
        >
          <p>
            Your own files and settings can be backed up now. Other users' profiles and some system locations need administrator rights; those items will be marked and skipped.
          </p>
          <Button
            className="mt-2"
            size="sm"
            variant="outline"
            onClick={() => backend.restartElevated(["other-user-profiles", "public-desktop"]).catch((e) => notify(errorMessage(e), "neutral"))}
          >
            Restart elevated (UAC prompt)
          </Button>
        </Alert>
      )}

      <Card className="p-5">
        <SectionTitle
          actions={
            scanning ? (
              <Button size="sm" variant="outline" onClick={() => backend.cancelScan()}>
                Cancel scan
              </Button>
            ) : scan ? (
              <Button size="sm" variant="outline" onClick={runScan} disabled={!authorized}>
                <RefreshCw className="h-4 w-4" /> Rescan
              </Button>
            ) : null
          }
        >
          3. Scan this PC
        </SectionTitle>
        {!scan && !scanning && (
          <div className="flex flex-col items-start gap-3">
            <p className="text-[13px] text-muted">The scan is read-only and runs in stages: computer, profiles, folders, browsers, Outlook, personalization, printers, drives and applications.</p>
            <Button variant="primary" size="lg" disabled={!authorized || !destination} onClick={runScan}>
              Start scan
            </Button>
            {!destination && <p className="text-[12px] text-warn">Choose a destination first.</p>}
          </div>
        )}
        {(scanning || stages.length > 0) && (
          <div className="space-y-3">
            <Progress value={pct} label="Scan progress" />
            <ol className="grid gap-1 sm:grid-cols-2" aria-live="polite">
              {stages.map((s) => {
                const done = !scanning || s.stage_index < (last?.stage_index ?? 0);
                return (
                  <li key={s.stage_index} className="flex items-center gap-2 text-[13px]">
                    {done ? <CheckCircle2 className="h-4 w-4 text-ok" aria-hidden /> : <Loader2 className="h-4 w-4 animate-spin text-accent" aria-hidden />}
                    <span className={done ? "text-muted" : ""}>{s.message}</span>
                  </li>
                );
              })}
              {scanning && stages.length === 0 && (
                <li className="flex items-center gap-2 text-[13px] text-muted">
                  <Circle className="h-4 w-4" /> Starting…
                </li>
              )}
            </ol>
          </div>
        )}
        {error && <Alert tone="danger" title="Scan failed">{error}</Alert>}
        {scan && !scanning && (
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-lg bg-surface-2 p-4">
            <p className="text-[13px]">
              Found <strong>{scan.items.length}</strong> items for <strong>{scan.users.filter((u) => !u.is_system_account).length}</strong> user profile(s), about{" "}
              <strong>{formatBytes(scan.items.reduce((a, i) => a + (i.estimated_size ?? 0), 0))}</strong> of data. {scan.warnings.length} notice(s).
            </p>
            <Button variant="primary" onClick={onContinue}>
              Review and select
            </Button>
          </div>
        )}
      </Card>
    </div>
  );
}
