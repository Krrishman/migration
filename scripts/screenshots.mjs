#!/usr/bin/env node
// Capture UI screenshots from the browser preview (mock backend with real
// fixture data). Usage:
//
//   npm run build && npm run screenshots
//
// Uses playwright-core with a locally installed Chromium. Set CHROMIUM_PATH
// if Chromium is not at /opt/pw-browsers/chromium.

import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "docs", "screenshots");
mkdirSync(out, { recursive: true });
const PORT = 4179;

// Run vite directly (not via npx) so killing the child stops the server.
const server = spawn(process.execPath, [join(root, "node_modules", "vite", "bin", "vite.js"), "preview", "--port", String(PORT), "--strictPort", "--host", "127.0.0.1"], { cwd: root, stdio: "pipe" });
const ready = new Promise((resolve, reject) => {
  const t = setTimeout(() => reject(new Error("preview server did not start")), 30000);
  server.stdout.on("data", (d) => {
    if (String(d).includes(String(PORT))) {
      clearTimeout(t);
      resolve();
    }
  });
});

const executablePath = process.env.CHROMIUM_PATH ?? "/opt/pw-browsers/chromium";

async function main() {
  await ready;
  const browser = await chromium.launch({ executablePath });
  try {
    await run(browser);
  } finally {
    await browser.close();
  }
}

async function run(browser) {
  const nav = (page, name) => page.getByRole("navigation", { name: "Categories" }).getByRole("button", { name });
  const shoot = async (page, name) => {
    await page.waitForTimeout(250);
    await page.screenshot({ path: join(out, `${name}.png`) });
    console.log("saved", name);
  };

  for (const scheme of ["light", "dark"]) {
    const ctx = await browser.newContext({ viewport: { width: 1366, height: 768 }, colorScheme: scheme, deviceScaleFactor: 1 });
    const page = await ctx.newPage();
    await page.goto(`http://127.0.0.1:${PORT}/`);
    await page.getByText("Authorized use only").waitFor();
    if (scheme === "light") await shoot(page, "01-authorization");
    await page.getByRole("checkbox", { name: "I am authorized" }).check();
    await page.getByRole("button", { name: "I understand and I am authorized" }).click();
    await page.getByText("Create migration backup").first().waitFor();
    await shoot(page, scheme === "light" ? "02-home" : "02-home-dark");
    if (scheme === "dark") {
      // Dashboard in dark mode.
      await page.getByRole("button", { name: /Create migration backup/ }).click();
      await page.getByRole("checkbox", { name: "Authorized to scan this device" }).check();
      await page.getByRole("button", { name: "Start scan" }).click();
      await page.getByRole("button", { name: "Review and select" }).click();
      await nav(page, /Users & Files/).click();
      await shoot(page, "04-dashboard-files-dark");
      await ctx.close();
      continue;
    }

    await page.getByRole("button", { name: /Create migration backup/ }).click();
    await page.getByRole("checkbox", { name: "Authorized to scan this device" }).check();
    await page.getByRole("button", { name: "Start scan" }).click();
    await page.getByRole("button", { name: "Review and select" }).waitFor();
    await shoot(page, "03-source-scan");
    await page.getByRole("button", { name: "Review and select" }).click();
    await shoot(page, "04-dashboard-overview");
    await nav(page, /Users & Files/).click();
    await page.getByRole("button", { name: "Documents" }).first().click();
    await shoot(page, "05-dashboard-files-details");
    await nav(page, /^Browsers/).click();
    await shoot(page, "06-dashboard-browsers");
    await nav(page, /Installed Applications/).click();
    await shoot(page, "07-dashboard-applications");
    await page.getByRole("button", { name: "Start backup" }).click();
    await page.getByText("Ready to capture?").waitFor();
    await shoot(page, "08-preflight");
    await page.getByRole("button", { name: /Continue and skip locked files|Start capture/ }).click();
    await page.waitForTimeout(1600);
    await shoot(page, "09-capture-progress");
    await page.getByRole("heading", { name: /Backup (complete|verified)/ }).waitFor({ timeout: 60000 });
    await shoot(page, "10-completion");

    await page.getByRole("button", { name: "Back to home" }).last().click();
    await page.getByRole("button", { name: /Restore migration backup/ }).click();
    await page.getByRole("button", { name: /Choose bundle folder/ }).click();
    await page.getByText("Bundle verified").waitFor();
    await shoot(page, "11-restore-verify");
    await page.getByRole("button", { name: "Continue" }).click();
    await shoot(page, "12-restore-mapping");
    await page.getByRole("button", { name: "Create dry-run plan" }).click();
    await page.getByText("Dry-run plan").waitFor();
    await shoot(page, "13-restore-dry-run");
    for (const cb of await page.getByRole("checkbox", { name: /^Confirm / }).all()) await cb.check();
    await page.getByRole("button", { name: "Restore confirmed categories" }).click();
    await page.getByRole("button", { name: "Restore now" }).click();
    await page.getByRole("heading", { name: /^Restore completed/ }).waitFor({ timeout: 60000 });
    await shoot(page, "14-restore-done");
    await ctx.close();
  }
}

main()
  .catch((e) => {
    console.error(e);
    process.exitCode = 1;
  })
  .finally(() => {
    server.kill();
    setTimeout(() => process.exit(process.exitCode ?? 0), 200);
  });
