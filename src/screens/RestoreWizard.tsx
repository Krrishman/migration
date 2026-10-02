import clsx from "clsx";
import { CheckCircle2, FileJson, FolderOpen, KeyRound, ShieldCheck, ShieldX } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { CategoryIcon, LogDrawer, TaskCard } from "../components/domain";
import { Alert, Badge, Button, Card, Checkbox, Dialog, Input, KeyValue, Progress, Select, Spinner } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes, formatDate } from "../lib/format";
import { CATEGORY_LABELS, CATEGORY_ORDER, POLICY_LABELS } from "../lib/labels";
import type { BundleOverview, Category, CollisionPolicy, LogEntry, RestorePlan, RestoreRequest, RestoreSummary, SystemChange, TaskProgress } from "../lib/types";
import { overall } from "./Capture";

type Step = "select" | "configure" | "plan" | "running" | "done";

export function describeChange(c: SystemChange): string {
  switch (c.type) {
    case "registry_value":
      return `Set registry value ${c.hive}\\${c.key}\\${c.value_name} = "${c.value}"`;
    case "map_drive":
      return `Map drive ${c.letter} to ${c.unc_path}${c.persistent ? " (reconnect at sign-in)" : ""}`;
    case "connect_shared_printer":
      return `Connect shared printer ${c.unc_path}`;
    case "add_network_printer":
      return `Add printer "${c.name}" on TCP/IP port ${c.port_name} (${c.host_address}) with installed driver "${c.driver_name}"`;
    case "manual_checklist":
      return `Manual step: ${c.text}`;
  }
}

const STEPS: { id: Step; label: string }[] = [
  { id: "select", label: "Open & verify" },
  { id: "configure", label: "Map users & choose items" },
  { id: "plan", label: "Dry run & confirm" },
  { id: "running", label: "Restore" },
  { id: "done", label: "Report" },
];

export function RestoreWizard() {
  const { backend, notify } = useApp();
  const [step, setStep] = useState<Step>("select");
  const [path, setPath] = useState<string>("");
  const [opening, setOpening] = useState(false);
  const [verified, setVerified] = useState(0);
  const [overview, setOverview] = useState<BundleOverview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [passphrase, setPassphrase] = useState("");
  const [mappings, setMappings] = useState<Record<string, string | null>>({});
  const [items, setItems] = useState<Set<string>>(new Set());
  const [policies, setPolicies] = useState<Partial<Record<Category, CollisionPolicy>>>({});
  const [replaceConfirmed, setReplaceConfirmed] = useState<Set<Category>>(new Set());
  const [plan, setPlan] = useState<RestorePlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [confirmed, setConfirmed] = useState<Set<Category>>(new Set());
  const [finalConfirm, setFinalConfirm] = useState(false);
  const [tasks, setTasks] = useState<Map<string, TaskProgress>>(new Map());
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [summary, setSummary] = useState<RestoreSummary | null>(null);

  const m = overview?.manifest;
  const restorable = useMemo(() => (m ? m.items.filter((i) => i.capture_status === "captured" || i.capture_status === "captured_with_warnings") : []), [m]);

  async function openBundle(p: string) {
    setPath(p);
    setOpening(true);
    setError(null);
    setVerified(0);
    const un = await backend.on<[number, string]>("restore://verify", ([n]) => setVerified(n));
    try {
      const o = await backend.openBundle(p);
      setOverview(o);
      setMappings(Object.fromEntries(o.suggested_mappings.map((x) => [x.source_sid, x.target_sid])));
      setItems(new Set(o.manifest.items.filter((i) => i.capture_status === "captured" || i.capture_status === "captured_with_warnings").map((i) => i.id)));
    } catch (e) {
      setError(errorMessage(e));
      setOverview(null);
    } finally {
      un();
      setOpening(false);
    }
  }

  const request = (): RestoreRequest => ({
    bundle_path: overview?.validation.bundle_path ?? path,
    mappings: Object.entries(mappings).map(([source_sid, target_sid]) => ({ source_sid, target_sid })),
    selected_item_ids: [...items],
    policies,
    replace_confirmed: [...replaceConfirmed],
    confirmed_categories: [...confirmed],
    passphrase: passphrase || undefined,
  });

  async function makePlan() {
    setPlanning(true);
    try {
      const p = await backend.planRestore(request());
      setPlan(p);
      setConfirmed(new Set());
      setStep("plan");
    } catch (e) {
      notify(errorMessage(e), "danger");
    } finally {
      setPlanning(false);
    }
  }

  async function runRestore() {
    setFinalConfirm(false);
    setStep("running");
    setTasks(new Map());
    setLogs([]);
    const unP = await backend.on<TaskProgress>("restore://progress", (p) =>
      setTasks((x) => {
        const n = new Map(x);
        n.set(p.task_id, p);
        return n;
      }),
    );
    const unL = await backend.on<LogEntry>("restore://log", (l) => setLogs((x) => [...x, l]));
    try {
      setSummary(await backend.executeRestore(request()));
      setStep("done");
    } catch (e) {
      notify(errorMessage(e), "danger");
      setStep("plan");
    } finally {
      unP();
      unL();
    }
  }

  const planCategories = useMemo(() => {
    const set = new Set<Category>();
    plan?.actions.filter((a) => a.requires_confirmation && !a.blocked_reason).forEach((a) => set.add(a.category));
    return CATEGORY_ORDER.filter((c) => set.has(c));
  }, [plan]);
  const categoriesWithItems = useMemo(() => CATEGORY_ORDER.filter((c) => restorable.some((i) => i.category === c)), [restorable]);
  const replaceUnconfirmed = categoriesWithItems.filter((c) => policies[c] === "replace_after_confirmation" && !replaceConfirmed.has(c));
  const needsPass = !!overview?.validation.encrypted;

  return (
    <div className="mx-auto max-w-6xl space-y-4 p-6">
      <ol className="flex flex-wrap gap-2" aria-label="Restore steps">
        {STEPS.map((s, i) => {
          const idx = STEPS.findIndex((x) => x.id === step);
          return (
            <li key={s.id} className={clsx("flex items-center gap-2 rounded-full px-3 py-1 text-[12px]", i === idx ? "bg-accent text-accent-fg" : i < idx ? "bg-ok/10 text-ok" : "bg-surface-2 text-muted")} aria-current={i === idx ? "step" : undefined}>
              {i < idx ? <CheckCircle2 className="h-3.5 w-3.5" /> : <span>{i + 1}</span>} {s.label}
            </li>
          );
        })}
      </ol>

      {step === "select" && (
        <Card className="space-y-4 p-5">
          <h2 className="text-lg font-semibold">Open a migration bundle</h2>
          <p className="text-[13px] text-muted">Choose the bundle folder (…\migrations\&lt;computer&gt;-&lt;date&gt;-&lt;id&gt;) or its manifest.json. Integrity is verified before any restore option is shown.</p>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" disabled={opening} onClick={async () => { const p = await backend.pickFolder("Choose the bundle folder"); if (p) openBundle(p); }}>
              <FolderOpen className="h-4 w-4" /> Choose bundle folder…
            </Button>
            <Button variant="outline" disabled={opening} onClick={async () => { const p = await backend.pickFile("Choose manifest.json", ["json"]); if (p) openBundle(p); }}>
              <FileJson className="h-4 w-4" /> Choose manifest.json…
            </Button>
          </div>
          {opening && (
            <div className="space-y-2">
              <Spinner label={`Verifying bundle integrity… ${verified ? `${verified} file(s) checked` : ""}`} />
              <Progress value={null} label="Verifying bundle" />
            </div>
          )}
          {error && <Alert tone="danger" title="This bundle cannot be opened">{error}</Alert>}
          {overview && m && (
            <div className="space-y-4">
              <Alert
                tone={overview.validation.ok ? "ok" : "danger"}
                icon={overview.validation.ok ? <ShieldCheck className="h-5 w-5 text-ok" /> : <ShieldX className="h-5 w-5 text-danger" />}
                title={overview.validation.ok ? "Bundle verified" : "Bundle failed verification — restore is blocked"}
              >
                <p>
                  Schema {overview.validation.schema_version} · manifest hash {overview.validation.manifest_hash_ok ? "matches" : "DOES NOT match"} · {overview.validation.files_checked} file(s) re-hashed ·{" "}
                  {overview.validation.mismatches.length} mismatch(es) · {overview.validation.missing.length} missing
                </p>
                {[...overview.validation.errors, ...overview.validation.mismatches.map((x) => `Hash mismatch: ${x}`), ...overview.validation.missing.map((x) => `Missing: ${x}`)].slice(0, 12).map((x) => (
                  <p key={x} className="font-mono text-[12px] text-danger">{x}</p>
                ))}
              </Alert>
              <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                <KeyValue label="Source computer" value={m.source_machine.computer_name} />
                <KeyValue label="Source Windows" value={`${m.source_machine.os_name} ${m.source_machine.os_version}`} />
                <KeyValue label="Captured" value={formatDate(m.created_at)} />
                <KeyValue label="Size" value={`${formatBytes(m.integrity.total_bytes)} · ${m.integrity.total_files} files`} />
                <KeyValue label="This PC" value={`${overview.target.computer_name} (${overview.target.os_name} ${overview.target.os_version})`} />
                <KeyValue label="Free space on this PC" value={formatBytes(overview.target.free_bytes_system_drive)} />
                <KeyValue label="Encryption" value={overview.validation.encrypted ? <Badge tone="accent"><KeyRound className="h-3.5 w-3.5" /> Passphrase required</Badge> : "Not encrypted"} />
                <KeyValue label="Previous restores" value={m.restore_history.length} />
              </dl>
              <div className="flex justify-end">
                <Button variant="primary" disabled={!overview.validation.ok} onClick={() => setStep("configure")}>
                  Continue
                </Button>
              </div>
            </div>
          )}
        </Card>
      )}

      {step === "configure" && overview && m && (
        <>
          {needsPass && (
            <Card className="space-y-2 p-5">
              <h3 className="font-semibold">Bundle passphrase</h3>
              <Input type="password" autoComplete="off" className="max-w-md" placeholder="Enter the passphrase used during capture" aria-label="Bundle passphrase" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
              <p className="text-[12px] text-muted">The passphrase is used only in memory to decrypt files and is never stored or logged.</p>
            </Card>
          )}
          <Card className="space-y-3 p-5">
            <h3 className="font-semibold">Map source users to users on this PC</h3>
            {!overview.target.elevated && <p className="text-[12px] text-muted">Running as a standard user: you can restore into your own profile. Restoring into other profiles requires restarting elevated.</p>}
            <table className="w-full text-[13px]">
              <thead className="text-left text-[12px] text-muted">
                <tr>
                  <th className="py-1 font-medium">Source user ({m.source_machine.computer_name})</th>
                  <th className="font-medium">Destination user ({overview.target.computer_name})</th>
                </tr>
              </thead>
              <tbody>
                {m.users.map((u) => (
                  <tr key={u.sid} className="border-t border-border">
                    <td className="py-2">
                      <p className="font-medium">{u.account_name}</p>
                      <p className="font-mono text-[11px] text-muted">{u.profile_path}</p>
                    </td>
                    <td>
                      <Select aria-label={`Destination for ${u.account_name}`} value={mappings[u.sid] ?? ""} onChange={(e) => setMappings({ ...mappings, [u.sid]: e.target.value || null })}>
                        <option value="">Do not restore this user</option>
                        {overview.target.profiles.filter((p) => !p.is_system_account && p.profile_exists).map((p) => (
                          <option key={p.sid} value={p.sid}>
                            {p.account_name}
                            {p.is_current_user ? " (you)" : ""}
                          </option>
                        ))}
                      </Select>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
          <Card className="space-y-4 p-5">
            <h3 className="font-semibold">Items and collision policy</h3>
            {categoriesWithItems.map((c) => (
              <section key={c} className="rounded-lg border border-border p-3" aria-label={CATEGORY_LABELS[c]}>
                <div className="flex flex-wrap items-center gap-2">
                  <CategoryIcon category={c} className="text-accent" />
                  <h4 className="font-medium">{CATEGORY_LABELS[c]}</h4>
                  <label className="ml-auto flex items-center gap-2 text-[12px]">
                    If a file already exists
                    <Select aria-label={`Collision policy for ${CATEGORY_LABELS[c]}`} value={policies[c] ?? "skip_existing"} onChange={(e) => setPolicies({ ...policies, [c]: e.target.value as CollisionPolicy })}>
                      {(Object.keys(POLICY_LABELS) as CollisionPolicy[]).map((p) => (
                        <option key={p} value={p}>{POLICY_LABELS[p]}</option>
                      ))}
                    </Select>
                  </label>
                </div>
                {policies[c] === "replace_after_confirmation" && (
                  <label className="mt-2 flex items-start gap-2 rounded bg-warn/10 p-2 text-[12px]">
                    <Checkbox label={`Confirm replace for ${CATEGORY_LABELS[c]}`} checked={replaceConfirmed.has(c)} onChange={(e) => { const n = new Set(replaceConfirmed); if (e.target.checked) n.add(c); else n.delete(c); setReplaceConfirmed(n); }} />
                    I confirm that existing files in {CATEGORY_LABELS[c]} may be replaced. Each existing file is first renamed to “name.pre-migration-&lt;timestamp&gt;.bak”; nothing is deleted.
                  </label>
                )}
                <ul className="mt-2 grid gap-1 sm:grid-cols-2">
                  {restorable.filter((i) => i.category === c).map((i) => (
                    <li key={i.id}>
                      <label className="flex items-center gap-2 text-[13px]">
                        <Checkbox label={`Restore ${i.display_name}`} checked={items.has(i.id)} onChange={(e) => { const n = new Set(items); if (e.target.checked) n.add(i.id); else n.delete(i.id); setItems(n); }} />
                        <span className="truncate">{i.display_name}</span>
                        {i.owner && <span className="truncate text-[11px] text-muted">({i.owner.account_name})</span>}
                        <span className="ml-auto shrink-0 text-[11px] text-muted">{formatBytes(i.captured_bytes)}</span>
                      </label>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
            <div className="flex items-center justify-between gap-2">
              <Button variant="ghost" onClick={() => setStep("select")}>Back</Button>
              <div className="flex items-center gap-3">
                {replaceUnconfirmed.length > 0 && <span className="text-[12px] text-warn">Confirm “Replace” for {replaceUnconfirmed.map((c) => CATEGORY_LABELS[c]).join(", ")} or choose another policy.</span>}
                {needsPass && !passphrase && <span className="text-[12px] text-warn">Enter the passphrase.</span>}
                <Button variant="primary" busy={planning} disabled={items.size === 0 || replaceUnconfirmed.length > 0 || (needsPass && !passphrase)} onClick={makePlan}>
                  Create dry-run plan
                </Button>
              </div>
            </div>
          </Card>
        </>
      )}

      {step === "plan" && plan && (
        <Card className="space-y-4 p-5">
          <div className="flex flex-wrap items-end justify-between gap-2">
            <div>
              <h2 className="text-lg font-semibold">Dry-run plan</h2>
              <p className="text-[13px] text-muted">Nothing has been written yet. Review every action and system change, then confirm per category.</p>
            </div>
            <dl className="flex gap-6">
              <KeyValue label="Files" value={plan.total_files.toLocaleString()} />
              <KeyValue label="Size" value={formatBytes(plan.total_bytes)} />
              <KeyValue label="Conflicts" value={<span className={plan.total_conflicts ? "text-warn" : ""}>{plan.total_conflicts}</span>} />
              <KeyValue label="Free on this PC" value={formatBytes(plan.target_free_bytes)} />
            </dl>
          </div>
          {plan.warnings.map((w, i) => (
            <Alert key={i} tone={w.severity === "error" ? "danger" : "warn"}>{w.message}</Alert>
          ))}
          <div className="overflow-x-auto rounded-lg border border-border">
            <table className="w-full text-[13px]">
              <thead className="bg-surface-2 text-left text-[12px] text-muted">
                <tr>
                  <th className="px-3 py-2 font-medium">#</th>
                  <th className="font-medium">Action</th>
                  <th className="font-medium">Target</th>
                  <th className="font-medium">Files</th>
                  <th className="font-medium">Conflicts</th>
                  <th className="px-3 font-medium">Policy / status</th>
                </tr>
              </thead>
              <tbody>
                {plan.actions.map((a, i) => (
                  <tr key={a.id} className={clsx("border-t border-border align-top", a.blocked_reason && "opacity-70")}>
                    <td className="px-3 py-2 text-muted">{i + 1}</td>
                    <td className="py-2">
                      <div className="flex items-center gap-2 font-medium"><CategoryIcon category={a.category} className="text-muted" /> {a.display_name}</div>
                      {(a.source_user || a.target_user) && <p className="text-[11px] text-muted">{a.source_user} → {a.target_user}</p>}
                      {a.system_changes.map((c, n) => (
                        <p key={n} className={clsx("mt-1 rounded px-2 py-1 font-mono text-[11px]", c.type === "manual_checklist" ? "bg-surface-2" : "bg-warn/10 text-fg")}>{describeChange(c)}</p>
                      ))}
                      {a.notes.slice(0, 2).map((n) => <p key={n} className="text-[11px] text-muted">{n}</p>)}
                    </td>
                    <td className="max-w-[260px] truncate py-2 font-mono text-[11px]" title={a.target_path ?? ""}>{a.target_path ?? "—"}</td>
                    <td className="py-2">{a.files}</td>
                    <td className={clsx("py-2", a.conflicts > 0 && "text-warn")}>{a.conflicts}</td>
                    <td className="px-3 py-2">
                      {a.blocked_reason ? <span className="text-[12px] text-warn">{a.blocked_reason}</span> : a.requires_confirmation ? <Badge>{POLICY_LABELS[a.policy]}</Badge> : <Badge tone="accent">Report only</Badge>}
                      {a.requires_admin && <Badge tone="warn" className="ml-1">Admin</Badge>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <fieldset className="rounded-lg border border-border p-4">
            <legend className="px-1 text-[13px] font-semibold">Confirm what may be written to this PC</legend>
            <div className="grid gap-2 sm:grid-cols-2">
              {planCategories.map((c) => {
                const changes = plan.actions.filter((a) => a.category === c && !a.blocked_reason).flatMap((a) => a.system_changes).filter((x) => x.type !== "manual_checklist");
                return (
                  <label key={c} className="flex items-start gap-2 text-[13px]">
                    <Checkbox label={`Confirm ${CATEGORY_LABELS[c]}`} className="mt-0.5" checked={confirmed.has(c)} onChange={(e) => { const n = new Set(confirmed); if (e.target.checked) n.add(c); else n.delete(c); setConfirmed(n); }} />
                    <span>
                      {CATEGORY_LABELS[c]}: {changes.length ? `apply ${changes.length} system change(s) listed above` : "write files as planned"}
                    </span>
                  </label>
                );
              })}
            </div>
          </fieldset>
          <div className="flex items-center justify-between">
            <Button variant="ghost" onClick={() => setStep("configure")}>Back</Button>
            <div className="flex gap-2">
              <Button variant="outline" busy={planning} onClick={makePlan}>Refresh plan</Button>
              <Button variant="primary" disabled={confirmed.size === 0} onClick={() => setFinalConfirm(true)}>Restore confirmed categories</Button>
            </div>
          </div>
          <Dialog
            open={finalConfirm}
            onOpenChange={setFinalConfirm}
            title="Start restore?"
            description={`Writes to ${overview?.target.computer_name}. Unconfirmed categories and blocked actions are skipped.`}
            footer={<><Button variant="ghost" onClick={() => setFinalConfirm(false)}>Cancel</Button><Button variant="primary" onClick={runRestore}>Restore now</Button></>}
          >
            <ul className="ml-5 list-disc text-[13px]">
              {[...confirmed].map((c) => <li key={c}>{CATEGORY_LABELS[c]} — {POLICY_LABELS[policies[c] ?? "skip_existing"]}</li>)}
            </ul>
            <p className="text-[12px] text-muted">Existing files are never silently overwritten. A restore report is saved and the restore is recorded in the bundle's manifest history.</p>
          </Dialog>
        </Card>
      )}

      {(step === "running" || step === "done") && (
        <RestoreProgress tasks={[...tasks.values()]} logs={logs} running={step === "running"} onCancel={() => backend.cancelRestore()} />
      )}

      {step === "done" && summary && (
        <Card className="space-y-3 p-5">
          <div className="flex items-center gap-3">
            {summary.failures === 0 ? <CheckCircle2 className="h-7 w-7 text-ok" /> : <ShieldX className="h-7 w-7 text-danger" />}
            <h2 className="text-lg font-semibold">Restore {summary.outcome.toLowerCase()}</h2>
          </div>
          <dl className="grid gap-4 sm:grid-cols-3 lg:grid-cols-6">
            <KeyValue label="Written" value={summary.files_written} />
            <KeyValue label="Skipped (existing)" value={summary.files_skipped} />
            <KeyValue label="Renamed" value={summary.files_renamed} />
            <KeyValue label="Replaced (.bak kept)" value={summary.files_replaced} />
            <KeyValue label="Failures" value={<span className={summary.failures ? "text-danger" : ""}>{summary.failures}</span>} />
            <KeyValue label="Verified" value={summary.verified ? <Badge tone="ok">Yes</Badge> : <Badge tone="danger">No</Badge>} />
          </dl>
          {summary.warnings.slice(0, 20).map((w, i) => (
            <p key={i} className={clsx("text-[13px]", w.severity === "error" ? "text-danger" : w.severity === "warning" ? "text-warn" : "text-muted")}>{w.message}</p>
          ))}
          <div className="flex gap-2">
            <Button variant="primary" onClick={() => backend.openPath(summary.report_html).catch((e) => notify(errorMessage(e), "danger"))}>Open restore report</Button>
          </div>
        </Card>
      )}
    </div>
  );
}

function RestoreProgress({ tasks, logs, running, onCancel }: { tasks: TaskProgress[]; logs: LogEntry[]; running: boolean; onCancel: () => void }) {
  const o = overall(tasks);
  const [confirm, setConfirm] = useState(false);
  useEffect(() => {
    if (!running) setConfirm(false);
  }, [running]);
  return (
    <div className="space-y-3">
      <Card className="p-5">
        <div className="flex items-center justify-between">
          <p className="font-semibold">{running ? "Restoring…" : "Restore finished"} · {o.done} of {o.total} actions</p>
          {running && <Button variant="outline" size="sm" onClick={() => setConfirm(true)}>Cancel</Button>}
        </div>
        <Progress className="mt-3 h-3" value={o.pct} label="Overall restore progress" />
      </Card>
      <div className="grid gap-3 md:grid-cols-2">
        {tasks.map((t) => <TaskCard key={t.task_id} task={t} />)}
      </div>
      <LogDrawer logs={logs} />
      <Dialog
        open={confirm}
        onOpenChange={setConfirm}
        title="Cancel the restore?"
        description="Files already restored stay in place; the current file is finished or rolled back. Nothing is deleted."
        footer={<><Button variant="ghost" onClick={() => setConfirm(false)}>Continue restoring</Button><Button variant="danger" onClick={() => { setConfirm(false); onCancel(); }}>Cancel restore</Button></>}
      />
    </div>
  );
}
