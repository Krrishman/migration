import scanData from "../mocks/data/scan.json";
import { defaultSelection, isSafe, isSelectable, matchesFilter, selectAllSafe, selectionTotals } from "./selection";
import type { ScanResult } from "./types";

const scan = scanData as unknown as ScanResult;
const current = scan.users.find((u) => u.is_current_user)!;

describe("selection rules", () => {
  it("default selection never includes sensitive, opt-in, unsupported or other users' items", () => {
    const sel = defaultSelection(scan);
    expect(sel.size).toBeGreaterThan(5);
    for (const i of scan.items.filter((x) => sel.has(x.id))) {
      expect(i.sensitive).toBe(false);
      expect(i.opt_in_only).toBe(false);
      expect(i.support).not.toBe("unsupported");
      if (i.owner) expect(i.owner.sid).toBe(current.sid);
    }
  });

  it("privacy-sensitive items require explicit enabling", () => {
    const recent = scan.items.find((i) => i.sensitive)!;
    expect(recent).toBeDefined();
    expect(isSelectable(recent, false)).toBe(false);
    expect(isSelectable(recent, true)).toBe(true);
  });

  it("unsupported and inaccessible items are never selectable", () => {
    for (const i of scan.items.filter((x) => x.support === "unsupported" || x.access === "access_denied")) {
      expect(isSelectable(i, true)).toBe(false);
    }
  });

  it("select all safe items respects the other-users toggle", () => {
    const mine = selectAllSafe(scan, new Set(), false);
    const all = selectAllSafe(scan, new Set(), true);
    expect(all.size).toBeGreaterThan(mine.size);
    for (const id of mine) {
      const i = scan.items.find((x) => x.id === id)!;
      expect(isSafe(i, scan, false)).toBe(true);
      expect(i.opt_in_only || i.sensitive).toBe(false);
    }
    expect([...all].some((id) => id.includes("pst:"))).toBe(false);
  });

  it("filters by name, path and user", () => {
    const docs = scan.items.find((i) => i.display_name === "Documents")!;
    expect(matchesFilter(docs, "documents")).toBe(true);
    expect(matchesFilter(docs, "ann docu")).toBe(true);
    expect(matchesFilter(docs, "printer")).toBe(false);
  });

  it("totals ignore inventory-only sizes", () => {
    const t = selectionTotals(scan, defaultSelection(scan));
    expect(t.bytes).toBeGreaterThan(0);
    expect(t.unknownSize).toBe(0);
    expect(t.users).toBe(1);
  });
});
