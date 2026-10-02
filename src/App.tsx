import { ArrowLeft, Monitor, Moon, ShieldCheck, Sun, WifiOff } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Spinner } from "./components/ui";
import { errorMessage, getBackend, type Backend } from "./lib/api";
import { AppContext } from "./lib/context";
import { applyTheme, loadTheme, type ThemePref } from "./lib/theme";
import type { AppStatus, CaptureRequest, CaptureSummary, ScanResult } from "./lib/types";
import { AuthorizationDialog } from "./screens/Authorization";
import { CaptureScreen } from "./screens/Capture";
import { CompletionScreen } from "./screens/Completion";
import { Dashboard } from "./screens/Dashboard";
import { Home } from "./screens/Home";
import { ReportViewer } from "./screens/ReportViewer";
import { RestoreWizard } from "./screens/RestoreWizard";
import { SourceScan } from "./screens/SourceScan";

export type View =
  | { name: "home" }
  | { name: "scan" }
  | { name: "dashboard" }
  | { name: "capture"; request: CaptureRequest; resume?: { bundlePath: string; passphrase: string | null } }
  | { name: "complete"; summary: CaptureSummary }
  | { name: "restore" }
  | { name: "report"; path?: string };

const TITLES: Record<View["name"], string> = {
  home: "Home",
  scan: "Create migration backup",
  dashboard: "Select what to migrate",
  capture: "Capturing",
  complete: "Backup complete",
  restore: "Restore migration backup",
  report: "Open existing report",
};

export default function App() {
  const [backend, setBackend] = useState<Backend | null>(null);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ name: "home" });
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [theme, setTheme] = useState<ThemePref>(loadTheme());
  const [toast, setToast] = useState<{ message: string; tone: "ok" | "danger" | "neutral" } | null>(null);

  useEffect(() => applyTheme(theme), [theme]);

  useEffect(() => {
    getBackend()
      .then(async (b) => {
        setBackend(b);
        setStatus(await b.appStatus());
      })
      .catch((e) => setError(errorMessage(e)));
  }, []);

  const refreshStatus = useCallback(async () => {
    if (backend) setStatus(await backend.appStatus());
  }, [backend]);

  const notify = useCallback((message: string, tone: "ok" | "danger" | "neutral" = "neutral") => {
    setToast({ message, tone });
    window.setTimeout(() => setToast(null), 4500);
  }, []);

  if (error) {
    return (
      <div className="grid h-full place-items-center p-8">
        <p className="text-danger">Migration Assistant could not start: {error}</p>
      </div>
    );
  }
  if (!backend || !status) {
    return (
      <div className="grid h-full place-items-center">
        <Spinner label="Starting Migration Assistant…" />
      </div>
    );
  }

  const busy = view.name === "capture";
  const nextTheme: Record<ThemePref, ThemePref> = { system: "light", light: "dark", dark: "system" };
  const ThemeIcon = theme === "light" ? Sun : theme === "dark" ? Moon : Monitor;

  return (
    <AppContext.Provider value={{ backend, status, refreshStatus, notify }}>
      <div className="flex h-full flex-col">
        <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border bg-surface px-4">
          {view.name !== "home" && (
            <Button variant="ghost" size="sm" onClick={() => setView({ name: "home" })} disabled={busy} aria-label="Back to home">
              <ArrowLeft className="h-4 w-4" />
            </Button>
          )}
          <img src="/icon.png" alt="" className="h-6 w-6" />
          <div className="min-w-0">
            <h1 className="text-[15px] font-semibold leading-tight">Migration Assistant</h1>
            <p className="text-[12px] leading-tight text-muted">{TITLES[view.name]}</p>
          </div>
          <div className="ml-auto flex items-center gap-2">
            {status.fixture_mode && (
              <Badge tone="warn" title={status.platform}>
                {backend.mode === "mock" ? "Demo data (browser preview)" : "Fixture mode"}
              </Badge>
            )}
            <Badge tone={status.elevated ? "warn" : "neutral"} title="Process token elevation">
              <ShieldCheck className="h-3.5 w-3.5" aria-hidden /> {status.elevated ? "Administrator" : "Standard user"}
            </Badge>
            <Badge tone="neutral" title="Migration Assistant makes no network connections">
              <WifiOff className="h-3.5 w-3.5" aria-hidden /> Offline only
            </Badge>
            <Button variant="ghost" size="sm" onClick={() => setTheme(nextTheme[theme])} aria-label={`Theme: ${theme}. Switch theme`} title={`Theme: ${theme}`}>
              <ThemeIcon className="h-4 w-4" />
            </Button>
          </div>
        </header>
        <main className="min-h-0 flex-1 overflow-y-auto">
          {view.name === "home" && <Home onNavigate={setView} />}
          {view.name === "scan" && (
            <SourceScan
              scan={scan}
              onScanned={(s, sel) => {
                setScan(s);
                setSelection(sel);
              }}
              onContinue={() => setView({ name: "dashboard" })}
            />
          )}
          {view.name === "dashboard" && scan && (
            <Dashboard scan={scan} setScan={setScan} selection={selection} setSelection={setSelection} onStart={(request) => setView({ name: "capture", request })} onRescan={() => setView({ name: "scan" })} />
          )}
          {view.name === "capture" && <CaptureScreen request={view.request} resume={view.resume} onDone={(summary) => setView({ name: "complete", summary })} />}
          {view.name === "complete" && (
            <CompletionScreen
              summary={view.summary}
              onAnotherScan={() => {
                setScan(null);
                setView({ name: "scan" });
              }}
              onResume={(bundlePath, passphrase) => setView({ name: "capture", request: { destination_root: "", selected_item_ids: [], encryption: { enabled: false }, options: { skip_locked_files: true, include_browser_cache: false, verify_after_copy: true, max_retries: 3 } }, resume: { bundlePath, passphrase } })}
              onHome={() => setView({ name: "home" })}
            />
          )}
          {view.name === "restore" && <RestoreWizard />}
          {view.name === "report" && <ReportViewer initialPath={view.path} />}
        </main>
        {!status.authorization_acknowledged && <AuthorizationDialog />}
        {toast && (
          <div role="status" className={`fixed bottom-4 left-1/2 z-50 -translate-x-1/2 rounded-lg px-4 py-2 text-[13px] shadow-lg ${toast.tone === "danger" ? "bg-danger text-white" : toast.tone === "ok" ? "bg-ok text-white" : "bg-fg text-bg"}`}>
            {toast.message}
          </div>
        )}
      </div>
    </AppContext.Provider>
  );
}
