import clsx from "clsx";
import { AlertTriangle, ChevronDown, ChevronRight, FolderPlus, Globe, LayoutDashboard, Lock, RefreshCw, Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { CategoryIcon, FreeSpace, StatusChips } from "../components/domain";
import { Alert, Badge, Button, Card, Checkbox, Dialog, Input, KeyValue, Select } from "../components/ui";
import { errorMessage } from "../lib/api";
import { useApp } from "../lib/context";
import { formatBytes, formatDate, truncateMiddle } from "../lib/format";
import { ACCESS_LABELS, CATEGORY_LABELS, CATEGORY_ORDER } from "../lib/labels";
import { isSelectable, matchesFilter, selectAllSafe, selectionTotals } from "../lib/selection";
import type { CaptureOptions, CaptureRequest, Category, DiscoveryItem, DiskSpace, PreflightReport, ScanResult } from "../lib/types";

type Nav = "overview" | Category | "warnings";

interface Props {
  scan: ScanResult;
  setScan: (s: ScanResult) => void;
  selection: Set<string>;
  setSelection: (s: Set<string>) => void;
  onStart: (r: CaptureRequest) => void;
  onRescan: () => void;
}

const MIN_PASS = 12;

export function Dashboard({ scan, setScan, selection, setSelection, onStart, onRescan }: Props) {
  const { backend, status, notify } = useApp();
  const [nav, setNav] = useState<Nav>("overview");
  const [query, setQuery] = useState("");
  const [allowSensitive, setAllowSensitive] = useState(false);
  const [includeOthers, setIncludeOthers] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [destSpace, setDestSpace] = useState<DiskSpace | null>(null);
  const [options, setOptions] = useState<CaptureOptions>({ skip_locked_files: true, include_browser_cache: false, verify_after_copy: true, max_retries: 3 });
  const [encrypt, setEncrypt] = useState(false);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [preflight, setPreflight] = useState<PreflightReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [addFolder, setAddFolder] = useState<{ path: string; owner: string } | null>(null);
  const [addBrowser, setAddBrowser] = useState<{ path: string; name: string; owner: string } | null>(null);

  const destination = status.session_destination ?? "";
  useEffect(() => {
    if (destination) backend.diskSpace(destination).then(setDestSpace).catch(() => setDestSpace(null));
  }, [backend, destination]);

  const totals = useMemo(() => selectionTotals(scan, selection), [scan, selection]);
  const counts = useMemo(() => {
    const m = new Map<Category, { total: number; selected: number; warnings: number }>();
    for (const c of CATEGORY_ORDER) m.set(c, { total: 0, selected: 0, warnings: 0 });
    for (const i of scan.items) {
      const e = m.get(i.category)!;
      e.total++;
      if (selection.has(i.id)) e.selected++;
      e.warnings += i.warnings.filter((w) => w.severity !== "info").length;
    }
    return m;
  }, [scan, selection]);
  const allWarnings = useMemo(
    () => [
      ...scan.warnings.map((w) => ({ item: "Scan", w })),
      ...scan.items.flatMap((i) => i.warnings.filter((w) => w.severity !== "info").map((w) => ({ item: `${i.display_name}${i.owner ? ` (${i.owner.account_name})` : ""}`, w }))),
    ],
    [scan],
  );

  const toggle = (id: string, on: boolean) => {
    const next = new Set(selection);
    if (on) next.add(id);
    else next.delete(id);
    setSelection(next);
  };

  const passOk = !encrypt || (pass.length >= MIN_PASS && pass === pass2);
  const request = (): CaptureRequest => ({
    destination_root: destination,
    selected_item_ids: [...selection],
    encryption: encrypt ? { enabled: true, passphrase: pass } : { enabled: false },
    options,
  });

  async function runPreflight() {
    setChecking(true);
    try {
      setPreflight(await backend.capturePreflight(request()));
    } catch (e) {
      notify(errorMessage(e), "danger");
    } finally {
      setChecking(false);
    }
  }

  const visibleItems = (cat: Category) => scan.items.filter((i) => i.category === cat && matchesFilter(i, query));

  // Plain render functions (not nested components) so rows are not remounted
  // on every selection change, which would drop keyboard focus.
  const renderItemRow = (item: DiscoveryItem) => {
    const selectable = isSelectable(item, allowSensitive);
    const open = expanded === item.id;
    const src = item.source.kind === "path" ? item.source.path : item.source.reference;
    return (
      <li key={item.id} className={clsx("rounded-lg border bg-surface", selection.has(item.id) ? "border-accent/50" : "border-border")}>
        <div className="flex items-start gap-3 p-3">
          <Checkbox
            label={`Select ${item.display_name}`}
            className="mt-1"
            checked={selection.has(item.id)}
            disabled={!selectable && !selection.has(item.id)}
            onChange={(e) => toggle(item.id, e.target.checked)}
          />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <button className="flex items-center gap-1 text-left font-medium hover:underline" aria-expanded={open} onClick={() => setExpanded(open ? null : item.id)}>
                {open ? <ChevronDown className="h-4 w-4" aria-hidden /> : <ChevronRight className="h-4 w-4" aria-hidden />}
                {item.display_name}
              </button>
              {item.owner && <Badge tone="neutral">{item.owner.account_name}</Badge>}
              <StatusChips item={item} />
            </div>
            <p className="mt-0.5 truncate font-mono text-[12px] text-muted" title={src}>
              {truncateMiddle(src, 110)}
            </p>
          </div>
          <div className="w-40 shrink-0 text-right text-[12px]">
            <p className="font-medium text-fg">{item.support === "inventory_only" && item.estimated_size === null ? "Inventory" : formatBytes(item.estimated_size)}</p>
            <p className="text-muted">{item.item_count !== null ? `${item.item_count.toLocaleString()} item(s)` : "—"}</p>
            <p className={clsx("text-muted", item.access === "access_denied" && "text-danger")}>{ACCESS_LABELS[item.access]}</p>
          </div>
        </div>
        {open && (
          <div className="grid gap-4 border-t border-border bg-surface-2/50 p-4 text-[13px] md:grid-cols-3" data-testid="item-details">
            <div>
              <p className="mb-1 font-semibold">Included</p>
              <ul className="ml-4 list-disc text-muted">{item.includes.length ? item.includes.map((x) => <li key={x}>{x}</li>) : <li>Inventory record only</li>}</ul>
            </div>
            <div>
              <p className="mb-1 font-semibold">Excluded</p>
              <ul className="ml-4 list-disc text-muted">{item.excludes.length ? item.excludes.map((x) => <li key={x}>{x}</li>) : <li>Nothing beyond standard exclusions</li>}</ul>
            </div>
            <div>
              <p className="mb-1 font-semibold">On restore</p>
              <ul className="ml-4 list-disc text-muted">{item.restore_notes.map((x) => <li key={x}>{x}</li>)}</ul>
            </div>
            {item.warnings.length > 0 && (
              <div className="md:col-span-3">
                <p className="mb-1 font-semibold">Notices</p>
                <ul className="space-y-1">
                  {item.warnings.map((w, n) => (
                    <li key={n} className={clsx(w.severity === "warning" && "text-warn", w.severity === "error" && "text-danger")}>
                      {w.message}
                      {w.path && <span className="ml-1 font-mono text-[11px] text-muted">{w.path}</span>}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {!selectable && item.sensitive && <p className="text-warn md:col-span-3">Enable “Show privacy-sensitive items” to select this item.</p>}
          </div>
        )}
      </li>
    );
  };

  const renderItemList = (cat: Category) => {
    const items = visibleItems(cat);
    const groups = new Map<string, DiscoveryItem[]>();
    for (const i of items) {
      const k = i.owner?.account_name ?? "This computer";
      groups.set(k, [...(groups.get(k) ?? []), i]);
    }
    if (!items.length) return <p className="text-muted">{query ? "No items match the filter." : "Nothing was found in this category."}</p>;
    return (
      <div className="space-y-5">
        {[...groups.entries()].map(([owner, list]) => (
          <section key={owner} aria-label={owner}>
            <h4 className="mb-2 text-[12px] font-semibold uppercase tracking-wide text-muted">{owner}</h4>
            <ul className="space-y-2">
              {list.map((i) => renderItemRow(i))}
            </ul>
          </section>
        ))}
      </div>
    );
  };

  const nonSystemUsers = scan.users.filter((u) => !u.is_system_account && u.profile_exists);
  const summaryCards: [string, string][] = [
    ["Users", `${totals.users} of ${nonSystemUsers.length}`],
    ["Files selected", totals.files ? totals.files.toLocaleString() : "0"],
    ["Browser profiles", `${scan.items.filter((i) => i.category === "browsers" && selection.has(i.id)).length} of ${scan.browser_profiles.length}`],
    ["Printers", `${scan.items.filter((i) => i.category === "printers" && selection.has(i.id)).length} of ${scan.printers.length}`],
    ["Network drives", `${scan.items.filter((i) => i.category === "network_drives" && selection.has(i.id)).length} of ${scan.mapped_drives.length}`],
    ["Applications", `${scan.applications.length}`],
    ["Estimated size", formatBytes(totals.bytes)],
    ["Warnings", `${totals.warnings}`],
  ];
  const remaining = destSpace ? destSpace.free_bytes - totals.bytes : null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Header */}
      <div className="border-b border-border bg-surface px-5 py-3">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-2">
          <div>
            <p className="text-[12px] text-muted">Source computer</p>
            <p className="font-semibold">{scan.machine.computer_name}</p>
          </div>
          <div>
            <p className="text-[12px] text-muted">Windows</p>
            <p className="font-medium">
              {scan.machine.os_name} {scan.machine.os_version} · build {scan.machine.os_build}
            </p>
          </div>
          <Badge tone={scan.elevated ? "warn" : "neutral"}>{scan.elevated ? "Elevated" : "Standard user"}</Badge>
          <div className="min-w-0">
            <p className="text-[12px] text-muted">Destination</p>
            <p className="truncate font-mono text-[12px]" title={destination}>
              {destination || "Not chosen"}
            </p>
          </div>
          <div className="w-44">
            <FreeSpace label="Destination free" free={destSpace?.free_bytes} total={destSpace?.total_bytes} needed={totals.bytes} />
          </div>
          <Button size="sm" variant="ghost" className="ml-auto" onClick={onRescan}>
            <RefreshCw className="h-4 w-4" /> Rescan
          </Button>
        </div>
        <div className="mt-3 grid grid-cols-4 gap-2 lg:grid-cols-8" aria-label="Selection summary">
          {summaryCards.map(([k, v]) => (
            <div key={k} className="rounded-lg bg-surface-2 px-3 py-2">
              <p className="text-[11px] text-muted">{k}</p>
              <p className={clsx("truncate font-semibold", k === "Warnings" && totals.warnings > 0 && "text-warn")}>{v}</p>
            </div>
          ))}
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
        {/* Category navigation */}
        <nav className="scroll-thin w-56 shrink-0 overflow-y-auto border-r border-border bg-surface p-2" aria-label="Categories">
          {(["overview", ...CATEGORY_ORDER, "warnings"] as Nav[]).map((n) => {
            const c = n !== "overview" && n !== "warnings" ? counts.get(n) : null;
            return (
              <button
                key={n}
                onClick={() => setNav(n)}
                aria-current={nav === n ? "page" : undefined}
                className={clsx("flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-[13px]", nav === n ? "bg-accent/10 font-semibold text-accent" : "hover:bg-surface-2")}
              >
                {n === "overview" ? <LayoutDashboard className="h-4 w-4" /> : n === "warnings" ? <AlertTriangle className="h-4 w-4" /> : <CategoryIcon category={n} />}
                <span className="flex-1 truncate">{n === "overview" ? "Overview" : n === "warnings" ? "Warnings" : CATEGORY_LABELS[n]}</span>
                {c && c.total > 0 && (
                  <span className="text-[11px] text-muted">
                    {c.selected}/{c.total}
                  </span>
                )}
                {n === "warnings" && allWarnings.length > 0 && <Badge tone="warn">{allWarnings.length}</Badge>}
              </button>
            );
          })}
        </nav>

        {/* Main panel */}
        <section className="scroll-thin min-w-0 flex-1 overflow-y-auto p-5" aria-label="Items">
          <div className="mb-4 flex flex-wrap items-center gap-2">
            <div className="relative w-72">
              <Search className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-muted" aria-hidden />
              <Input className="pl-8" placeholder="Search items, paths, users…" aria-label="Search items" value={query} onChange={(e) => setQuery(e.target.value)} />
            </div>
            <Button size="sm" variant="outline" onClick={() => setSelection(selectAllSafe(scan, selection, includeOthers))}>
              Select all safe items
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setSelection(new Set())}>
              Clear selection
            </Button>
            <label className="ml-auto flex items-center gap-2 text-[12px]">
              <Checkbox label="Include other users in Select all" checked={includeOthers} onChange={(e) => setIncludeOthers(e.target.checked)} /> Include other users
            </label>
            <label className="flex items-center gap-2 text-[12px]">
              <Checkbox label="Show privacy-sensitive items" checked={allowSensitive} onChange={(e) => setAllowSensitive(e.target.checked)} /> Allow privacy-sensitive items
            </label>
          </div>

          {nav === "overview" && (
            <div className="space-y-5">
              <Card className="p-4">
                <h3 className="mb-3 font-semibold">User profiles</h3>
                <table className="w-full text-[13px]">
                  <thead className="text-left text-[12px] text-muted">
                    <tr>
                      <th className="py-1 font-medium">Account</th>
                      <th className="font-medium">Profile path</th>
                      <th className="font-medium">Last use</th>
                      <th className="font-medium">Approx. size</th>
                      <th className="font-medium">Status</th>
                    </tr>
                  </thead>
                  <tbody>
                    {scan.users.map((u) => (
                      <tr key={u.sid} className="border-t border-border">
                        <td className="py-2">
                          <span className="font-medium">{u.account_name}</span>
                          {u.is_current_user && <Badge tone="accent" className="ml-2">You</Badge>}
                        </td>
                        <td className="font-mono text-[12px]">{u.profile_path}</td>
                        <td title={`Confidence: ${u.last_use_confidence}`}>
                          {formatDate(u.last_use)} <span className="text-[11px] text-muted">({u.last_use_confidence})</span>
                        </td>
                        <td>{formatBytes(u.size_bytes)}</td>
                        <td>
                          {u.is_system_account ? <Badge>System — not offered</Badge> : u.access === "access_denied" ? <Badge tone="danger">Access denied</Badge> : !u.profile_exists ? <Badge>Missing</Badge> : <Badge tone="ok">Available</Badge>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <p className="mt-2 text-[12px] text-muted">Privacy notice: last-use times are approximate and shown only to help plan the migration.</p>
              </Card>
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {CATEGORY_ORDER.map((c) => {
                  const e = counts.get(c)!;
                  return (
                    <button key={c} onClick={() => setNav(c)} className="rounded-xl border border-border bg-surface p-4 text-left hover:border-accent/50">
                      <div className="flex items-center gap-2 font-semibold">
                        <CategoryIcon category={c} className="text-accent" /> {CATEGORY_LABELS[c]}
                      </div>
                      <p className="mt-1 text-[13px] text-muted">
                        {e.selected} of {e.total} selected
                        {e.warnings > 0 && <span className="text-warn"> · {e.warnings} warning(s)</span>}
                      </p>
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {nav !== "overview" && nav !== "warnings" && (
            <div className="space-y-4">
              <div className="flex items-center gap-2">
                <h3 className="text-lg font-semibold">{CATEGORY_LABELS[nav]}</h3>
                {nav === "users_files" && (
                  <Button size="sm" variant="outline" className="ml-auto" onClick={async () => {
                    const p = await backend.pickFolder("Choose a folder to include");
                    if (p) setAddFolder({ path: p, owner: scan.users.find((u) => u.is_current_user)?.sid ?? "" });
                  }}>
                    <FolderPlus className="h-4 w-4" /> Add folder…
                  </Button>
                )}
                {nav === "browsers" && (
                  <Button size="sm" variant="outline" className="ml-auto" onClick={() => setAddBrowser({ path: "", name: "", owner: scan.users.find((u) => u.is_current_user)?.sid ?? "" })}>
                    <Globe className="h-4 w-4" /> Add Chromium-family browser…
                  </Button>
                )}
              </div>
              {nav === "browsers" && (
                <Alert tone="accent" icon={<Lock className="h-4 w-4 text-accent" />} title="Passwords, cookies and sign-ins are never migrated">
                  Windows and browser encryption protect saved passwords, cookies, tokens and payment data; they are tied to this PC and user and cannot be moved. Ask the user to turn on
                  browser account sync (or use the browser's own password export) before migrating. Close browsers before capture.
                </Alert>
              )}
              {nav === "outlook_email" && (
                <Alert tone="neutral" title="Mailboxes re-sync from the server">
                  Account credentials are never copied. OST files are server caches and are excluded. PST files are opt-in and copied as files for the user to open in Outlook.
                </Alert>
              )}
              {nav === "personalization" && (
                <Alert tone="neutral" title="Best effort across Windows versions">
                  Wallpaper and theme files are copied reliably. Applying the wallpaper writes one documented registry value after confirmation. Start, taskbar and Quick Access layouts
                  are not guaranteed to restore.
                </Alert>
              )}
              {renderItemList(nav)}
              {nav === "installed_applications" && <AppsTable scan={scan} query={query} />}
              {nav === "system_inventory" && (
                <Card className="p-4">
                  <dl className="grid gap-3 sm:grid-cols-3">
                    <KeyValue label="CPU" value={`${scan.machine.cpu_summary} (${scan.machine.logical_cpus} threads)`} />
                    <KeyValue label="Memory" value={formatBytes(scan.machine.total_memory_bytes)} />
                    <KeyValue label="Architecture" value={scan.machine.architecture} />
                    <KeyValue label="Time zone" value={scan.machine.time_zone} />
                    <KeyValue label="Join state" value={`${scan.machine.join_state}${scan.machine.join_name ? ` (${scan.machine.join_name})` : ""}`} />
                    <KeyValue label="Network adapters" value={scan.machine.network_adapters.map((a) => a.name).join(", ") || "—"} />
                  </dl>
                  <h4 className="mt-4 text-[13px] font-semibold">Drives</h4>
                  <ul className="mt-1 text-[13px] text-muted">
                    {scan.machine.drives.map((d) => (
                      <li key={d.mount_point}>
                        {d.mount_point} {d.label} ({d.file_system}, {d.kind}) — {formatBytes(d.free_bytes)} free of {formatBytes(d.total_bytes)}
                      </li>
                    ))}
                  </ul>
                </Card>
              )}
            </div>
          )}

          {nav === "warnings" && (
            <div className="space-y-2">
              <h3 className="text-lg font-semibold">Warnings</h3>
              {allWarnings.length === 0 && <p className="text-muted">No warnings.</p>}
              <ul className="space-y-2">
                {allWarnings
                  .filter(({ item, w }) => matchesFilter({ display_name: item, description: w.message, source: { kind: "config", reference: w.path ?? "" } } as DiscoveryItem, query))
                  .map(({ item, w }, n) => (
                    <li key={n} className="rounded-lg border border-border bg-surface p-3 text-[13px]">
                      <p className="font-medium">{item}</p>
                      <p className={clsx(w.severity === "warning" && "text-warn", w.severity === "error" && "text-danger", w.severity === "info" && "text-muted")}>{w.message}</p>
                      {w.path && <p className="font-mono text-[11px] text-muted">{w.path}</p>}
                    </li>
                  ))}
              </ul>
            </div>
          )}
        </section>

        {/* Migration plan drawer */}
        <aside className="scroll-thin flex w-80 shrink-0 flex-col overflow-y-auto border-l border-border bg-surface" aria-label="Migration plan">
          <div className="space-y-4 p-4">
            <h3 className="font-semibold">Migration plan</h3>
            <ul className="space-y-1 text-[13px]">
              {CATEGORY_ORDER.filter((c) => counts.get(c)!.selected > 0).map((c) => (
                <li key={c} className="flex items-center gap-2">
                  <CategoryIcon category={c} className="text-muted" />
                  <span className="flex-1">{CATEGORY_LABELS[c]}</span>
                  <span className="text-muted">{counts.get(c)!.selected}</span>
                </li>
              ))}
              {selection.size === 0 && <li className="text-muted">Nothing selected yet.</li>}
            </ul>
            <dl className="grid grid-cols-2 gap-3 rounded-lg bg-surface-2 p-3">
              <KeyValue label="Estimated size" value={formatBytes(totals.bytes)} />
              <KeyValue label="Selected items" value={totals.count} />
              <KeyValue label="Destination free" value={formatBytes(destSpace?.free_bytes)} />
              <KeyValue label="Remaining after" value={<span className={clsx(remaining !== null && remaining < 0 && "text-danger")}>{remaining === null ? "Unknown" : formatBytes(Math.max(0, remaining))}</span>} />
            </dl>
            {totals.unknownSize > 0 && <p className="text-[12px] text-warn">{totals.unknownSize} item(s) have no size estimate.</p>}
            {totals.warnings > 0 && (
              <button className="text-[12px] text-warn underline" onClick={() => setNav("warnings")}>
                {totals.warnings} warning(s) in the selection — review
              </button>
            )}

            <fieldset className="space-y-2 rounded-lg border border-border p-3 text-[13px]">
              <legend className="px-1 text-[12px] font-semibold text-muted">Options</legend>
              <label className="flex items-center gap-2">
                <Checkbox label="Verify every file after copying" checked={options.verify_after_copy} onChange={(e) => setOptions({ ...options, verify_after_copy: e.target.checked })} />
                Verify every file after copying (SHA-256)
              </label>
              <label className="flex items-center gap-2">
                <Checkbox label="Skip locked files" checked={options.skip_locked_files} onChange={(e) => setOptions({ ...options, skip_locked_files: e.target.checked })} />
                Skip files in use (after retries) and report them
              </label>
              <label className="flex items-center gap-2">
                <Checkbox label="Include browser caches" checked={options.include_browser_cache} onChange={(e) => setOptions({ ...options, include_browser_cache: e.target.checked })} />
                Include browser caches (not recommended)
              </label>
            </fieldset>

            <fieldset className="space-y-2 rounded-lg border border-border p-3 text-[13px]">
              <legend className="px-1 text-[12px] font-semibold text-muted">Encryption at rest</legend>
              <label className="flex items-center gap-2">
                <Checkbox label="Encrypt the bundle" checked={encrypt} onChange={(e) => setEncrypt(e.target.checked)} />
                Encrypt files (AES-256-GCM, Argon2id)
              </label>
              {encrypt && (
                <div className="space-y-2">
                  <Input type="password" autoComplete="new-password" placeholder={`Passphrase (min. ${MIN_PASS} characters)`} aria-label="Passphrase" value={pass} onChange={(e) => setPass(e.target.value)} />
                  <Input type="password" autoComplete="new-password" placeholder="Confirm passphrase" aria-label="Confirm passphrase" value={pass2} onChange={(e) => setPass2(e.target.value)} />
                  {pass.length > 0 && pass.length < MIN_PASS && <p className="text-[12px] text-warn">At least {MIN_PASS} characters.</p>}
                  {pass2.length > 0 && pass !== pass2 && <p className="text-[12px] text-danger">Passphrases do not match.</p>}
                  <p className="text-[12px] text-danger">If the passphrase is lost, the encrypted data cannot be restored by anyone. File names and the inventory stay readable.</p>
                </div>
              )}
              {!encrypt && <p className="text-[12px] text-muted">Without encryption anyone with the drive can read the copied files. Store the drive securely.</p>}
            </fieldset>

            <Button variant="primary" size="lg" className="w-full" disabled={selection.size === 0 || !destination || !passOk} busy={checking} onClick={runPreflight}>
              Start backup
            </Button>
            {!destination && <p className="text-[12px] text-warn">Choose a destination on the previous step.</p>}
          </div>
        </aside>
      </div>

      {/* Preflight dialog */}
      <Dialog
        open={!!preflight}
        onOpenChange={(o) => !o && setPreflight(null)}
        title="Ready to capture?"
        description="Preflight checks for the destination, free space and open applications."
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setPreflight(null)}>
              Cancel
            </Button>
            <Button variant="outline" busy={checking} onClick={runPreflight}>
              Retry detection
            </Button>
            <Button variant="primary" disabled={!preflight || preflight.blocking_errors.length > 0} onClick={() => onStart(request())}>
              {preflight?.running_apps.length ? "Continue and skip locked files" : "Start capture"}
            </Button>
          </>
        }
      >
        {preflight && (
          <>
            <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <KeyValue label="Selected" value={preflight.selected_count} />
              <KeyValue label="Estimated" value={formatBytes(preflight.estimated_bytes)} />
              <KeyValue label="Destination free" value={formatBytes(preflight.destination_free_bytes)} />
              <KeyValue label="File system" value={preflight.destination_file_system ?? "Unknown"} />
            </dl>
            {preflight.blocking_errors.map((b) => (
              <Alert key={b} tone="danger" title="Cannot start">
                {b}
              </Alert>
            ))}
            {preflight.running_apps.length > 0 && (
              <Alert tone="warn" title="Close these applications first">
                <p>{preflight.running_apps.join(", ")} {preflight.running_apps.length === 1 ? "is" : "are"} running. Ask the user to close them, then choose Retry detection.</p>
                <p className="mt-1">Migration Assistant never force-closes applications. If you continue, files that stay locked are skipped and listed in the report.</p>
              </Alert>
            )}
            {preflight.warnings
              .filter((w) => w.code !== "application_running")
              .map((w, i) => (
                <Alert key={i} tone={w.severity === "info" ? "neutral" : "warn"}>
                  {w.message}
                </Alert>
              ))}
            <p className="text-[12px] text-muted">
              Source data is only read, never changed or deleted. The bundle is written to <span className="font-mono">{preflight.destination_root}\migrations\</span>.
            </p>
          </>
        )}
      </Dialog>

      {/* Add custom folder */}
      <Dialog
        open={!!addFolder}
        onOpenChange={(o) => !o && setAddFolder(null)}
        title="Add a folder"
        description="System folders (Windows, Program Files) and credential stores are rejected automatically."
        footer={
          <>
            <Button variant="ghost" onClick={() => setAddFolder(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={async () => {
                if (!addFolder) return;
                try {
                  const item = await backend.addCustomFolder(addFolder.path, addFolder.owner || null);
                  setScan({ ...scan, items: scan.items.some((i) => i.id === item.id) ? scan.items : [...scan.items, item] });
                  setSelection(new Set([...selection, item.id]));
                  setAddFolder(null);
                  notify(`Added ${addFolder.path}`, "ok");
                } catch (e) {
                  notify(errorMessage(e), "danger");
                }
              }}
            >
              Add folder
            </Button>
          </>
        }
      >
        {addFolder && (
          <>
            <p className="font-mono text-[13px]">{addFolder.path}</p>
            <label className="block text-[13px]">
              Belongs to user (used for mapping on restore)
              <Select className="mt-1 w-full" value={addFolder.owner} onChange={(e) => setAddFolder({ ...addFolder, owner: e.target.value })}>
                <option value="">This computer (restored to the target user's Migrated Files)</option>
                {nonSystemUsers.map((u) => (
                  <option key={u.sid} value={u.sid}>
                    {u.account_name}
                  </option>
                ))}
              </Select>
            </label>
          </>
        )}
      </Dialog>

      {/* Add Chromium-family browser */}
      <Dialog
        open={!!addBrowser}
        onOpenChange={(o) => !o && setAddBrowser(null)}
        title="Add a Chromium-family browser"
        description="Select the browser's “User Data” folder (for example …\BraveSoftware\Brave-Browser\User Data). Only the same safe files as Chrome are captured."
        footer={
          <>
            <Button variant="ghost" onClick={() => setAddBrowser(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!addBrowser?.path || !addBrowser?.name.trim() || !addBrowser.owner}
              onClick={async () => {
                if (!addBrowser) return;
                try {
                  const items = await backend.addChromiumRoot(addBrowser.path, addBrowser.name, addBrowser.owner);
                  setScan({ ...scan, items: [...scan.items, ...items] });
                  setAddBrowser(null);
                  notify(`Found ${items.length} profile(s). They are opt-in; tick the ones to capture.`, "ok");
                } catch (e) {
                  notify(errorMessage(e), "danger");
                }
              }}
            >
              I confirm this is the browser's profile folder
            </Button>
          </>
        }
      >
        {addBrowser && (
          <>
            <Input placeholder="Browser name, e.g. Brave" aria-label="Browser name" value={addBrowser.name} onChange={(e) => setAddBrowser({ ...addBrowser, name: e.target.value })} />
            <Select className="w-full" aria-label="User" value={addBrowser.owner} onChange={(e) => setAddBrowser({ ...addBrowser, owner: e.target.value })}>
              {nonSystemUsers.map((u) => (
                <option key={u.sid} value={u.sid}>
                  {u.account_name}
                </option>
              ))}
            </Select>
            <div className="flex items-center gap-2">
              <Input readOnly value={addBrowser.path} placeholder="No folder chosen" aria-label="Profile root" className="font-mono text-[12px]" />
              <Button
                variant="outline"
                onClick={async () => {
                  const p = await backend.pickFolder("Choose the browser's User Data folder");
                  if (p) setAddBrowser({ ...addBrowser, path: p });
                }}
              >
                Browse…
              </Button>
            </div>
          </>
        )}
      </Dialog>
    </div>
  );
}

function AppsTable({ scan, query }: { scan: ScanResult; query: string }) {
  const q = query.toLowerCase();
  const apps = scan.applications.filter((a) => !q || [a.display_name, a.publisher ?? "", a.category].join(" ").toLowerCase().includes(q));
  return (
    <Card className="overflow-hidden">
      <table className="w-full text-[13px]">
        <thead className="bg-surface-2 text-left text-[12px] text-muted">
          <tr>
            <th className="px-3 py-2 font-medium">Application</th>
            <th className="font-medium">Version</th>
            <th className="font-medium">Publisher</th>
            <th className="font-medium">Category</th>
            <th className="font-medium">Arch.</th>
            <th className="px-3 font-medium">Settings plug-in</th>
          </tr>
        </thead>
        <tbody>
          {apps.map((a) => (
            <tr key={`${a.display_name}-${a.version}`} className="border-t border-border align-top">
              <td className="px-3 py-2">
                <p className="font-medium">{a.display_name}</p>
                <p className="text-[12px] text-muted">{a.description ?? "Unknown application."}</p>
              </td>
              <td className="py-2">{a.version ?? "—"}</td>
              <td className="py-2">{a.publisher ?? "—"}</td>
              <td className="py-2">{a.category}</td>
              <td className="py-2">{a.architecture}</td>
              <td className="px-3 py-2">{a.settings_plugin ? <Badge tone="teal">Available</Badge> : <span className="text-muted">—</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="border-t border-border px-3 py-2 text-[12px] text-muted">Applications are never copied. Reinstall them from official sources; a checklist is included in the bundle.</p>
    </Card>
  );
}
