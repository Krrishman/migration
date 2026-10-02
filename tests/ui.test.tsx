// UI flow tests against the mock backend (real fixture data).
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "../src/App";
import { overall } from "../src/screens/Capture";
import { setBackend } from "../src/lib/api";
import { MockBackend } from "../src/mocks/mockBackend";
import type { TaskProgress } from "../src/lib/types";

beforeAll(() => {
  // jsdom lacks these browser APIs used by the UI.
  window.matchMedia = window.matchMedia ?? ((() => ({ matches: false, addEventListener() {}, removeEventListener() {} })) as unknown as typeof window.matchMedia);
  globalThis.structuredClone = globalThis.structuredClone ?? ((v: unknown) => JSON.parse(JSON.stringify(v)));
});

function fastBackend() {
  const b = new MockBackend();
  b.speed = 1000;
  setBackend(b);
  return b;
}

async function acceptAuthorization() {
  await screen.findByText("Authorized use only");
  const accept = screen.getByRole("button", { name: "I understand and I am authorized" });
  expect(accept).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox", { name: "I am authorized" }));
  fireEvent.click(accept);
  await waitFor(() => expect(screen.queryByText("Authorized use only")).not.toBeInTheDocument());
}

describe("Migration Assistant UI", () => {
  it("requires authorization before use and shows portable status", async () => {
    fastBackend();
    render(<App />);
    await acceptAuthorization();
    expect(screen.getByText("Portable mode")).toBeInTheDocument();
    expect(screen.getByText(/No network access, no telemetry/)).toBeInTheDocument();
    expect(screen.getByText("Demo data (browser preview)")).toBeInTheDocument();
  });

  it("scans, selects safe items and runs a capture to a truthful completion", async () => {
    fastBackend();
    render(<App />);
    await acceptAuthorization();
    fireEvent.click(screen.getByRole("button", { name: /Create migration backup/ }));
    const start = screen.getByRole("button", { name: "Start scan" });
    expect(start).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Authorized to scan this device" }));
    fireEvent.click(start);
    fireEvent.click(await screen.findByRole("button", { name: "Review and select" }));

    const nav = screen.getByRole("navigation", { name: "Categories" });
    fireEvent.click(within(nav).getByRole("button", { name: /Desktop & Shortc/ }));
    const recent = screen.getByRole("checkbox", { name: /Select Recent items/ });
    expect(recent).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Show privacy-sensitive items" }));
    expect(screen.getByRole("checkbox", { name: /Select Recent items/ })).not.toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Clear selection" }));
    expect(screen.getByRole("button", { name: "Start backup" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Select all safe items" }));
    expect(screen.getByRole("checkbox", { name: /Select Recent items/ })).not.toBeChecked();

    // Encryption requires a confirmed passphrase of sufficient length.
    fireEvent.click(screen.getByRole("checkbox", { name: "Encrypt the bundle" }));
    expect(screen.getByRole("button", { name: "Start backup" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Passphrase"), { target: { value: "correct horse battery" } });
    fireEvent.change(screen.getByLabelText("Confirm passphrase"), { target: { value: "correct horse battery" } });
    fireEvent.click(screen.getByRole("button", { name: "Start backup" }));
    await screen.findByText("Ready to capture?");
    expect(screen.getByText(/never force-closes applications/)).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Continue and skip locked files|Start capture/ }));
    });
    await screen.findByRole("heading", { name: /Backup (complete and verified|verified, with warnings)/ }, { timeout: 15000 });
    expect(screen.getAllByTestId("task-card").every((c) => ["completed", "completed_with_warnings", "skipped"].includes(c.dataset.state!))).toBe(true);
    expect(screen.getByRole("button", { name: /Prepare restore instructions/ })).toBeInTheDocument();
  }, 30000);

  it("restore requires verification, a dry run and per-category confirmation", async () => {
    fastBackend();
    render(<App />);
    await acceptAuthorization();
    fireEvent.click(screen.getByRole("button", { name: /Restore migration backup/ }));
    fireEvent.click(screen.getByRole("button", { name: /Choose bundle folder/ }));
    await screen.findByText("Bundle verified");
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    // Replace policy needs an extra confirmation before planning.
    fireEvent.change(screen.getByLabelText("Collision policy for Users & Files"), { target: { value: "replace_after_confirmation" } });
    expect(screen.getByRole("button", { name: "Create dry-run plan" })).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Confirm replace for Users & Files" }));
    fireEvent.click(screen.getByRole("button", { name: "Create dry-run plan" }));
    await screen.findByText("Dry-run plan");
    expect(screen.getByText(/Map drive H: to/)).toBeInTheDocument();
    expect(screen.getAllByText(/Set registry value HKCU\\Control Panel\\Desktop\\WallPaper/).length).toBeGreaterThan(0);
    const go = screen.getByRole("button", { name: "Restore confirmed categories" });
    expect(go).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Confirm Users & Files" }));
    fireEvent.click(go);
    await act(async () => {
      fireEvent.click(await screen.findByRole("button", { name: "Restore now" }));
    });
    await screen.findByRole("heading", { name: /^Restore / }, { timeout: 15000 });
  }, 30000);
});

describe("overall progress", () => {
  const t = (state: TaskProgress["state"], done: number, total: number | null): TaskProgress => ({
    task_id: Math.random().toString(),
    category: "users_files",
    display_name: "x",
    state,
    current_path: null,
    bytes_done: done,
    bytes_total: total,
    items_done: 0,
    items_total: null,
    bytes_per_second: null,
    elapsed_ms: 0,
    eta_seconds: null,
    warning_count: 0,
    retry_count: 0,
    error: null,
  });
  it("is derived from task outcomes", () => {
    expect(overall([t("completed", 10, 10), t("copying", 5, 10)]).pct).toBe(75);
    expect(overall([t("completed", 10, 10), t("scanning", 0, null)]).pct).toBe(50);
    expect(overall([]).pct).toBe(0);
  });
});
