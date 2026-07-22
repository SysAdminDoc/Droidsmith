/* global window, document, getComputedStyle, HTMLElement, requestAnimationFrame */
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { argv, env, execPath, platform, stdout } from "node:process";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const uiSmokePort =
  env.DROIDSMITH_UI_SMOKE_PORT ?? String(await findOpenPort());
const baseUrl = `http://127.0.0.1:${uiSmokePort}`;
const screenshotDir = path.join(repoRoot, "test-results", "rendered-routes");
const docsScreenshotDir = path.join(repoRoot, "docs", "screenshots");
// When set, refresh the committed README screenshots from the mocked-native
// state instead of only asserting they are free of desktop-required placeholders.
const captureDocs = argv.includes("--capture-docs");

fs.mkdirSync(screenshotDir, { recursive: true });

const server = startVite();

try {
  await waitForHttp(baseUrl);

  const browser = await chromium.launch();
  try {
    await runDesktopFlow(browser);
    await runMobileFlow(browser);
    await runLocaleZoomFlow(browser);
    await runResilienceFlow(browser);
    await runWatcherFallbackFlow(browser);
    await runDocCaptureFlow(browser);
  } finally {
    await browser.close();
  }

  stdout.write("Rendered route smoke OK\n");
} finally {
  stopServer(server);
}

async function runDesktopFlow(browser) {
  const page = await browser.newPage({
    viewport: { width: 1366, height: 900 },
  });
  const errors = collectConsoleErrors(page);
  await installTauriMock(page);
  await page.goto(baseUrl, { waitUntil: "networkidle" });

  await page.getByRole("heading", { name: "Droidsmith" }).waitFor();
  await page.getByText("ADB lifecycle health", { exact: true }).waitFor();
  await page.getByText("ADB ready", { exact: true }).waitFor();
  await page.getByText(/Platform Tools 37\.0\.0 makes libadbmdns/).waitFor();
  await page.getByRole("button", { name: "Select Pixel QA" }).waitFor();
  if ((await page.locator("tr[role='button']").count()) !== 0) {
    throw new Error("Device table rows must retain native table semantics");
  }
  await page.getByRole("button", { name: "More device options" }).click();
  await page.getByRole("menuitem", { name: "Review recovery" }).click();
  await page
    .getByRole("dialog", { name: "Review ADB restart and reconnect" })
    .waitFor();
  await page.getByText("adb kill-server", { exact: true }).waitFor();
  await page.getByText("adb reconnect offline", { exact: true }).waitFor();
  await assertTabMovesFocus(page, "ADB recovery review");
  await page.getByRole("button", { name: "Confirm and run" }).click();
  await page
    .getByText("ADB restarted and the offline reconnect request completed.")
    .waitFor();
  await page.getByLabel("Copyable diagnostics").waitFor({ state: "visible" });
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-adb-health-recovery.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Close", exact: true }).click();

  // Read-only layout inspector: select the device, capture the mocked UI
  // hierarchy, confirm nodes render indented, and export the raw XML.
  await page.getByRole("button", { name: "Select Pixel QA" }).click();
  await page.getByRole("heading", { name: "Layout inspector" }).waitFor();
  await page.getByRole("button", { name: "Capture hierarchy" }).click();
  await page.getByText("com.example.app:id/title").waitFor();
  await page.getByText("“Hello”").waitFor();
  // IMP-71: filtering to zero matches shows an empty state, not a blank panel.
  const layoutFilter = page.getByLabel("Filter layout nodes");
  await layoutFilter.fill("zzz-no-such-node");
  await page.getByText("No matching nodes", { exact: true }).waitFor();
  await layoutFilter.fill("");
  await page.getByText("com.example.app:id/title").waitFor();
  await page.getByRole("button", { name: "Export XML" }).click();
  await page.getByText(/Layout saved to .*layout-QA123\.xml/).waitFor();

  // Persist gnirehtet across navigation: the Share Internet panel re-attaches to
  // an already-running session on mount (shows Active + Stop, not Start).
  const tetherPanel = page
    .getByRole("heading", { name: "Share Internet", exact: true })
    .locator("../../..");
  await tetherPanel.getByText("Active", { exact: true }).waitFor();
  await tetherPanel.getByRole("button", { name: "Stop Sharing" }).click();
  await page.getByText("Internet sharing stopped.", { exact: true }).waitFor();

  await page.getByRole("button", { name: "About", exact: true }).click();
  await page.getByRole("button", { name: "Diagnostics", exact: true }).click();
  const diagnosticsDialog = page.getByRole("dialog", {
    name: "Diagnostics center",
  });
  await diagnosticsDialog.waitFor();
  await page.getByText("Nothing is uploaded", { exact: true }).waitFor();
  await diagnosticsDialog
    .getByRole("button", { name: "Run connection doctor" })
    .click();
  await diagnosticsDialog
    .getByText("Custom ADB server configuration is active", { exact: true })
    .waitFor();
  await diagnosticsDialog.getByText(/value redacted/).waitFor();
  const diagnosticsPreview = page.getByLabel("Redacted bundle preview");
  await diagnosticsPreview.waitFor();
  const diagnosticsValue = await diagnosticsPreview.inputValue();
  if (
    !diagnosticsValue.includes('"uploads_performed": false') ||
    !diagnosticsValue.includes("device-01") ||
    diagnosticsValue.includes("QA123")
  ) {
    throw new Error(
      "Diagnostics preview did not enforce the redacted local-only contract",
    );
  }
  await page.getByRole("button", { name: "Save bundle" }).click();
  await page
    .getByText(/Saved .* support bundle to .*droidsmith-support\.json/)
    .waitFor();
  await diagnosticsDialog
    .getByRole("button", { name: "Review privacy warning" })
    .click();
  await diagnosticsDialog
    .getByText(/dumpsys and dumpstate output, Logcat history, screenshots/)
    .waitFor();
  await diagnosticsDialog
    .getByRole("checkbox", {
      name: /I understand the likely contents/,
    })
    .check();
  await diagnosticsDialog
    .getByRole("button", { name: "Choose destination and capture" })
    .click();
  await diagnosticsDialog
    .getByText("Sensitive bugreport saved locally", { exact: true })
    .waitFor();
  await diagnosticsDialog
    .getByText(/droidsmith-bugreport-2026-07-15\.zip\.metadata\.json/)
    .waitFor();
  await page
    .getByRole("button", { name: "Wipe local diagnostic history" })
    .click();
  await page
    .getByRole("alertdialog", { name: "Wipe erasable diagnostic history?" })
    .waitFor();
  await assertTabMovesFocus(page, "Diagnostics wipe review");
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-diagnostics-center.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Close", exact: true }).click();

  await page.getByRole("button", { name: "Select Pixel QA" }).click();
  await page.getByRole("heading", { name: "File manager" }).waitFor();
  await page.getByRole("button", { name: "Browse", exact: true }).click();
  await page.getByText("Résumé final.txt", { exact: true }).waitFor();

  await page.getByRole("button", { name: "New folder" }).click();
  await page.getByRole("dialog", { name: "Create device folder" }).waitFor();
  await page.getByLabel("Name", { exact: true }).fill("Project notes");
  await page.getByRole("button", { name: "Review file change" }).click();
  const mkdirReview = page.getByRole("alertdialog", {
    name: "Review file change",
  });
  await mkdirReview.waitFor();
  await mkdirReview
    .getByText('adb shell "mkdir" "/sdcard/Project notes"', { exact: true })
    .waitFor();
  await assertTabMovesFocus(page, "File mkdir review");
  await mkdirReview.getByRole("button", { name: "Confirm and run" }).click();
  await page.getByText("Project notes/", { exact: true }).waitFor();

  const unicodeRow = page
    .getByText("Résumé final.txt", { exact: true })
    .locator("..");
  await unicodeRow.getByRole("button", { name: "Rename" }).click();
  await page.getByLabel("Name", { exact: true }).fill("Résumé archived.txt");
  await page.getByRole("button", { name: "Review file change" }).click();
  const renameReview = page.getByRole("alertdialog", {
    name: "Review file change",
  });
  await renameReview
    .getByText(
      'adb shell "mv" "-n" "/sdcard/Résumé final.txt" "/sdcard/Résumé archived.txt"',
      { exact: true },
    )
    .waitFor();
  await renameReview.getByRole("button", { name: "Confirm and run" }).click();
  await page.getByText("Résumé archived.txt", { exact: true }).waitFor();

  const protectedRow = page
    .getByText("Protected notes.txt", { exact: true })
    .locator("..");
  await protectedRow.getByRole("button", { name: "Delete" }).click();
  const deleteReview = page.getByRole("alertdialog", {
    name: "Review file change",
  });
  await deleteReview
    .getByText('adb shell "rm" "-f" "/sdcard/Protected notes.txt"', {
      exact: true,
    })
    .waitFor();
  await deleteReview.getByRole("button", { name: "Confirm and run" }).click();
  await page.getByText(/Permission denied.*left available/).waitFor();
  await deleteReview.getByRole("button", { name: "Cancel" }).click();

  await page.getByRole("button", { name: "Push file" }).click();
  const pushReview = page.getByRole("alertdialog", {
    name: "Review file change",
  });
  await pushReview.waitFor();
  await pushReview
    .getByText(
      'adb push "C:/Users/QA/Downloads/新しい report.txt" "/sdcard/新しい report.txt"',
      { exact: true },
    )
    .waitFor();
  await pushReview.getByRole("button", { name: "Confirm and run" }).click();
  await page.getByText("新しい report.txt", { exact: true }).waitFor();
  await page.screenshot({
    path: path.join(
      screenshotDir,
      "desktop-file-manager-guarded-operations.png",
    ),
    fullPage: false,
  });

  for (const route of ["Devices", "Apps", "Debloat", "Profiles", "Console"]) {
    await page.getByRole("button", { name: new RegExp(route) }).click();
    await page.getByRole("heading", { name: route, exact: true }).waitFor();
    await assertMainFocused(page, route);
  }

  await page.getByLabel("ADB shell command").fill("getprop ro.product.model");
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await page.getByText("Pixel QA", { exact: true }).waitFor();
  await page.getByLabel("ADB shell command").fill("rm /sdcard/qa.txt");
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await page
    .getByRole("alertdialog", { name: "Review shell mutation" })
    .waitFor();
  await page.getByText("Dangerous mutation", { exact: true }).waitFor();
  await assertTabMovesFocus(page, "Console mutation review");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-console-mutation-review.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Cancel", exact: true }).click();

  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_TRANSPORT__("unknown_tcp"),
  );
  await page
    .getByRole("heading", { name: "Unauthenticated wireless transport" })
    .waitFor();
  await page.getByLabel("ADB shell command").fill("getprop ro.product.model");
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await page
    .getByText("untrusted_transport_override_required", { exact: true })
    .waitFor();
  await page
    .getByLabel(
      "Allow privileged operations over this connection until I select another device.",
    )
    .check();
  await page.getByLabel("ADB shell command").fill("getprop ro.product.model");
  await page.getByRole("button", { name: "Run", exact: true }).click();
  await page.getByText("Pixel QA", { exact: true }).waitFor();
  const privilegedTargetAccepted = await page.evaluate(
    () => window.__DROIDSMITH_LAST_PRIVILEGED_TARGET__,
  );
  if (
    privilegedTargetAccepted?.transport_kind !== "unknown_tcp" ||
    privilegedTargetAccepted?.untrusted_transport_override !== true
  ) {
    throw new Error(
      "Unsafe transport acknowledgement was not scoped to the live target",
    );
  }
  await page.evaluate(() => window.__DROIDSMITH_MOCK_TRANSPORT__("usb"));
  await page
    .getByRole("heading", { name: "Unauthenticated wireless transport" })
    .waitFor({ state: "hidden" });

  await page.getByRole("button", { name: /Logcat/ }).click();
  await page.getByRole("heading", { name: "Logcat", exact: true }).waitFor();
  await assertMainFocused(page, "Logcat");
  await page.getByRole("button", { name: "Start tail" }).click();
  await page.getByText(/I\/QA\(\s*123\): first part complete/).waitFor();
  await page
    .getByText(/Logcat disconnected; reconnecting \(attempt 2\)/)
    .waitFor();
  await page.getByText(/W\/QA\(\s*123\): after reconnect/).waitFor();
  const logOutput = page.getByRole("log", { name: "Logcat output" });
  if ((await logOutput.getAttribute("aria-live")) !== "off") {
    throw new Error("Logcat output must not announce every appended line");
  }
  await page.waitForFunction(() =>
    Array.from(document.querySelectorAll("[role='status']")).some((element) =>
      /Logcat updated; \d+ visible lines?\./.test(element.textContent ?? ""),
    ),
  );
  await assertNoHorizontalOverflow(page, "desktop Logcat stream");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-logcat-stream.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.getByRole("button", { name: "Start tail" }).waitFor();

  await page.getByRole("button", { name: /Apps/ }).click();
  await page.getByText("com.example.app").waitFor();
  await page.getByText("Example App", { exact: true }).waitFor();
  await page.getByText("com.android.settings").waitFor();
  // R-087: an OTA fingerprint change surfaces a dismissible debloat-drift notice.
  const otaNotice = page
    .getByText("Device updated since last use", { exact: true })
    .locator("../..");
  await otaNotice.waitFor();
  await otaNotice.getByRole("button", { name: "Dismiss" }).click();
  await page
    .getByText("Device updated since last use", { exact: true })
    .waitFor({ state: "detached" });
  const exampleAppRow = page
    .getByRole("row")
    .filter({ hasText: "com.example.app" });
  await exampleAppRow.getByRole("button", { name: "Export APKs" }).click();
  await page.getByText("APK export verified", { exact: true }).waitFor();
  await page.getByText(/Exported and hashed 2 base\/split APK files/).waitFor();
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  await page.getByRole("button", { name: "Show advanced data export" }).click();
  await exampleAppRow.getByRole("button", { name: "Legacy data…" }).click();
  await page
    .getByText("Review deprecated data export", { exact: true })
    .waitFor();
  await page.getByRole("button", { name: "Continue legacy export" }).click();
  await page.getByText("Legacy archive inspected", { exact: true }).waitFor();
  await page
    .getByText(/Restore compatibility and completeness are not verified/)
    .waitFor();
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(false));
  await page.getByText("No authorized devices", { exact: true }).waitFor();
  await page.getByText("com.example.app").waitFor({ state: "hidden" });
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(true));
  await page.getByText("com.example.app").waitFor();

  // IMP-62: the package table is an ARIA grid with roving-tabindex cell
  // navigation. Drive it from the keyboard and assert the tab stop moves.
  const packageGrid = page.getByRole("grid", { name: "Installed packages" });
  await packageGrid.waitFor();
  const activeCell = () =>
    page.evaluate(
      () => document.activeElement?.getAttribute("data-grid-cell") ?? null,
    );
  await page.evaluate(() =>
    document.querySelector('[data-grid-cell="0-0"]')?.focus(),
  );
  await page.keyboard.press("ArrowDown");
  if ((await activeCell()) !== "1-0") {
    throw new Error(
      `Grid ArrowDown should move the tab stop to cell 1-0, got ${await activeCell()}`,
    );
  }
  await page.keyboard.press("ArrowRight");
  if ((await activeCell()) !== "1-1") {
    throw new Error(
      `Grid ArrowRight should move the tab stop to cell 1-1, got ${await activeCell()}`,
    );
  }
  await page.keyboard.press("End");
  if ((await activeCell()) !== "1-4") {
    throw new Error(
      `Grid End should move the tab stop to the last cell 1-4, got ${await activeCell()}`,
    );
  }
  await page.keyboard.press("Enter");
  const enteredTag = await page.evaluate(
    () => document.activeElement?.tagName ?? null,
  );
  if (enteredTag !== "BUTTON") {
    throw new Error(
      `Grid Enter should hand focus to the cell's control, got ${enteredTag}`,
    );
  }

  // Opt-in incremental install records and surfaces its mode.
  await page.getByRole("checkbox", { name: "Incremental" }).check();
  await page.getByRole("button", { name: "Install package" }).click();
  await page.getByText(/Installed incrementally/).waitFor();
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  await page.getByRole("checkbox", { name: "Incremental" }).uncheck();

  await page.getByRole("button", { name: "Install package" }).click();
  await page
    .getByText("INSTALL_FAILED_VERSION_DOWNGRADE", { exact: true })
    .waitFor();
  await page.getByRole("button", { name: "Review guarded override" }).click();
  await page
    .getByRole("alertdialog", { name: "Confirm an unsafe install override" })
    .waitFor();
  await page.getByText(/retries with -d/).waitFor();
  await assertTabMovesFocus(page, "Apps install override review");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-apps-install-override.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Install with override" }).click();
  await page.getByText("Package installed", { exact: true }).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Apps table");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-apps-table.png"),
    fullPage: false,
  });

  await page.getByRole("button", { name: "Inspect recovery baseline" }).click();
  await page.getByText("Read-only recovery diff", { exact: true }).waitFor();
  await page.getByText("Build changed / OTA drift", { exact: true }).waitFor();
  await page.getByText("Skipped safely", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Apply 1 reviewed actions" }).click();
  await page.getByText("Applied: 1; failed: 0.", { exact: true }).waitFor();
  await page
    .getByRole("button", { name: "Close recovery baseline", exact: true })
    .click();

  await page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .getByRole("button", { name: "Disable" })
    .click();
  await page.getByRole("alertdialog").waitFor();
  await page.getByText(/pm disable-user --user 0 com\.example\.app/).waitFor();
  await assertTabMovesFocus(page, "Apps action overlay");
  await page
    .getByRole("button", { name: "Export pre-change baseline" })
    .click();
  await page
    .getByText(/Portable recovery evidence saved to .*droidsmith-recovery/)
    .waitFor();
  await page.getByRole("button", { name: "Cancel" }).click();
  await assertFocusedButton(page, "Disable");
  await page
    .getByRole("button", { name: "Close recovery baseline", exact: true })
    .click();

  await page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .getByRole("button", { name: "Disable" })
    .click();
  await page.getByRole("alertdialog").waitFor();
  await page.getByRole("button", { name: "Apply change" }).click();
  await page
    .getByRole("dialog", { name: "Applying change" })
    .waitFor({ state: "visible" });
  await page.getByText("Action completed", { exact: true }).waitFor();
  await page
    .getByRole("status")
    .filter({
      has: page.getByRole("heading", { name: "Action completed", exact: true }),
    })
    .getByRole("button", { name: "Dismiss", exact: true })
    .click();

  const archiveActionRow = page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .filter({ has: page.getByRole("button", { name: "Archive" }) });
  await archiveActionRow.getByRole("button", { name: "Archive" }).click();
  const archiveReview = page.getByRole("alertdialog", {
    name: "Apply package action",
  });
  await archiveReview.waitFor();
  await archiveReview
    .getByText(/pm archive --user 0 com\.example\.app/)
    .waitFor();
  if (
    (await archiveReview
      .getByRole("button", {
        name: "Export pre-change baseline",
      })
      .count()) !== 0
  ) {
    throw new Error("Archive review exposed an incompatible portable baseline");
  }
  await archiveReview.getByRole("button", { name: "Apply change" }).click();
  await page.getByText("Action completed", { exact: true }).waitFor();
  await page
    .getByRole("status")
    .filter({
      has: page.getByRole("heading", { name: "Action completed", exact: true }),
    })
    .getByRole("button", { name: "Dismiss", exact: true })
    .click();
  const archivedPackageRow = page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .filter({ has: page.getByRole("button", { name: "Unarchive" }) });
  await archivedPackageRow.getByText("Archived", { exact: true }).waitFor();

  const archiveJournalRow = page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .filter({ hasText: "Archive" });
  await archiveJournalRow.getByRole("button", { name: "Undo" }).click();
  await page.getByText("Undo completed for com.example.app.").waitFor();
  const restoredPackageRow = page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .filter({ has: page.getByRole("button", { name: "Enable" }) });
  await restoredPackageRow.getByText("Disabled", { exact: true }).waitFor();
  await page.evaluate(() => window.__DROIDSMITH_MOCK_ARCHIVE_API__(34));
  await page.getByRole("button", { name: "Refresh packages" }).click();
  await page
    .getByRole("heading", { name: "App archiving unavailable" })
    .waitFor();
  if (
    (await restoredPackageRow
      .getByRole("button", { name: "Archive" })
      .count()) !== 0
  ) {
    throw new Error("Android 14 exposed an unsupported archive action");
  }
  await page.evaluate(() => window.__DROIDSMITH_MOCK_ARCHIVE_API__(35));

  await page.getByRole("button", { name: /Profiles/ }).click();
  await page.getByRole("heading", { name: "Profiles", exact: true }).waitFor();
  await page.getByLabel("Name", { exact: true }).fill("QA profile");
  await page
    .getByLabel("Search packages for profile actions")
    .fill("com.example.app");
  await page.getByRole("checkbox", { name: /com\.example\.app/ }).check();
  await page.getByRole("button", { name: "Add selected (1)" }).click();
  await page.getByRole("button", { name: "Validate and export" }).click();
  await page.getByText("Profile saved", { exact: true }).waitFor();
  await page.getByRole("tab", { name: "Import and preview" }).click();
  await page.getByRole("button", { name: "Choose profile" }).click();
  await page.getByText("Full dry-run diff", { exact: true }).waitFor();
  await page
    .getByText("Explicit migration required: v1 to v2", { exact: true })
    .waitFor();
  await page.getByText("Already matches", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Save reviewed v2 profile" }).click();
  await page.getByText(/Validated schema v2 profile saved to/).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Profiles diff");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-profiles-diff.png"),
    fullPage: false,
  });

  await page.keyboard.press("Control+k");
  await page.getByRole("dialog", { name: "Command palette" }).waitFor();
  await assertFocusedLabel(page, "Command palette search");
  const paletteInput = page.getByRole("combobox", {
    name: "Command palette search",
  });
  const controlledListbox = await paletteInput.getAttribute("aria-controls");
  if (
    !controlledListbox ||
    (await page.locator(`#${controlledListbox}`).count()) !== 1
  ) {
    throw new Error("Command palette combobox does not control its listbox");
  }
  const firstActiveOption = await paletteInput.getAttribute(
    "aria-activedescendant",
  );
  await paletteInput.press("ArrowDown");
  const secondActiveOption = await paletteInput.getAttribute(
    "aria-activedescendant",
  );
  if (!firstActiveOption || firstActiveOption === secondActiveOption) {
    throw new Error("Command palette did not expose its active option");
  }
  await paletteInput.fill("debloat");
  await page.waitForFunction(
    (selector) =>
      document
        .querySelector(selector)
        ?.getAttribute("aria-activedescendant") === "command-palette-option-0",
    '[role="combobox"]',
  );
  await paletteInput.press("Enter");
  await page.getByRole("heading", { name: "Debloat", exact: true }).waitFor();
  await assertMainFocused(page, "Debloat");

  // R-098: export the current device's debloat state to a shareable pack.
  await page.getByRole("button", { name: "Export to pack…" }).click();
  await page.getByText(/Exported 2 packages to .qa-device-export./).waitFor();

  await page.getByRole("button", { name: /QA Debloat Pack/ }).click();
  await page.getByRole("heading", { name: "Compatibility checks" }).waitFor();
  await page.getByText(/Pack qa-debloat · revision 3 · MIT/).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Debloat pack preview");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-debloat-preview.png"),
    fullPage: false,
  });
  // IMP-63: a curated preset pre-checks only its matching packages. "Privacy
  // Max" matches the telemetry-labelled com.example.app (1 of the pack).
  await page.getByRole("button", { name: /Privacy Max/ }).click();
  await page.getByRole("button", { name: /Apply 1 package/ }).waitFor();
  // Restore the broader selection the rest of this flow exercises.
  await page.getByRole("checkbox", { name: /com\.example\.fail/ }).check();
  await page.getByRole("checkbox", { name: /com\.android\.settings/ }).check();
  await page
    .getByRole("button", { name: "Export baseline before applying" })
    .click();
  await page.getByText(/Baseline saved to .*qa-debloat/).waitFor();
  await page.getByRole("button", { name: /Apply 3 packages/ }).click();
  const debloatReview = page.getByRole("alertdialog", {
    name: "Confirm debloat changes",
  });
  await debloatReview.waitFor();
  await debloatReview
    .getByText("com.android.settings", { exact: true })
    .waitFor();
  await debloatReview.getByText("3", { exact: true }).waitFor();
  await debloatReview.getByText("1", { exact: true }).waitFor();
  await assertTabMovesFocus(page, "Debloat final safety review");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-debloat-final-review.png"),
    fullPage: false,
  });
  const debloatDisable = debloatReview.getByRole("button", {
    name: "Disable 3 packages",
  });
  await debloatDisable.waitFor();
  if (await debloatDisable.isDisabled()) {
    await debloatReview
      .getByRole("checkbox", { name: /I understand these unsafe-tier/ })
      .check();
  } else {
    throw new Error(
      "Unsafe-tier debloat selection did not gate the confirm button",
    );
  }
  await debloatDisable.click();
  // The completion title uses an em-dash separator ("{{name}} — debloat
  // complete"); match any single separator char so the assertion survives
  // dash-style normalization in the locale files.
  await page.getByText(/QA Debloat Pack . debloat complete/).waitFor();
  await page.getByText("Failed", { exact: true }).waitFor();
  await page.getByRole("button", { name: /Retry 1 failed/ }).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Debloat queue");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-debloat-queue.png"),
    fullPage: false,
  });

  // IMP-68: the failed row can explain itself via the bundled quirk rules.
  await page.getByRole("button", { name: "Why did this fail?" }).click();
  await page
    .getByText("Vendor blocks disabling this package", { exact: true })
    .waitFor();

  // R-097: the offline APK analyzer renders a manifest/dex/signing report.
  await page.getByRole("button", { name: /APK Analyzer/ }).click();
  await page
    .getByRole("heading", { name: "APK Analyzer", exact: true })
    .waitFor();
  await page.getByRole("button", { name: "Choose an APK" }).click();
  await page.getByText("com.example.qa", { exact: true }).waitFor();
  await page.getByText("Multidex / 64K+", { exact: true }).waitFor();
  await page.getByText("Signed (v2, v3)", { exact: true }).waitFor();
  await page
    .getByText("android.permission.POST_NOTIFICATIONS", { exact: true })
    .waitFor();
  await assertNoHorizontalOverflow(page, "desktop APK Analyzer report");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-apk-analyzer.png"),
    fullPage: false,
  });

  await page.getByRole("button", { name: /Wireless/ }).click();
  await page.getByRole("heading", { name: "Wireless", exact: true }).waitFor();
  const wirelessConnectPanel = page
    .getByRole("heading", { name: "Paired endpoint", exact: true })
    .locator("..")
    .locator("..");
  await wirelessConnectPanel.getByLabel("Host").fill("pixel.local");
  await wirelessConnectPanel.getByLabel("Port").fill("38899");
  await wirelessConnectPanel
    .getByRole("button", { name: "Connect", exact: true })
    .click();
  await page
    .getByText("An active VPN or tunnel may be blocking the device route", {
      exact: true,
    })
    .waitFor();
  const wirelessDiagnostics = page.getByLabel(
    "Copyable wireless failure diagnostics",
  );
  await wirelessDiagnostics.waitFor();
  const wirelessDiagnosticsValue = await wirelessDiagnostics.inputValue();
  if (
    !wirelessDiagnosticsValue.includes('"active_vpn_interfaces": 1') ||
    wirelessDiagnosticsValue.includes("pixel.local")
  ) {
    throw new Error(
      "Wireless diagnostics did not enforce the privacy-bounded evidence contract",
    );
  }
  await assertNoHorizontalOverflow(page, "desktop Wireless failure hint");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-wireless-failure-hint.png"),
    fullPage: false,
  });

  // R-086: the connection-history panel surfaces a saved endpoint with a
  // one-click reconnect and the opt-in reconnect-on-launch toggle.
  await page.getByRole("heading", { name: "Connection history" }).waitFor();
  await page.getByText("192.168.1.42:5555", { exact: true }).waitFor();
  const wirelessAutoReconnect = page.getByRole("checkbox", {
    name: "Reconnect known devices on launch",
  });
  await wirelessAutoReconnect.check();
  await wirelessAutoReconnect.uncheck();
  await page
    .getByRole("row", { name: /192\.168\.1\.42:5555/ })
    .getByRole("button", { name: "Reconnect" })
    .waitFor();

  await page.getByRole("button", { name: /Mirror/ }).click();
  await page.getByRole("heading", { name: "Mirror", exact: true }).waitFor();
  await page.getByText("scrcpy 4.0", { exact: true }).waitFor();
  // R-088: the expanded scrcpy flag surface is present.
  await page
    .getByRole("checkbox", { name: "View only (no control)" })
    .waitFor();
  await page.getByLabel("Max FPS").fill("60");
  // R-089: validation-heavier flags are exposed and selectable.
  await page.getByLabel("Crop (w:h:x:y)").fill("1224:1440:0:0");
  await page.getByLabel("Display orientation").selectOption("90");
  await page.getByLabel("Audio codec").selectOption("opus");
  // R-089: version-gated virtual display + audio-source picker.
  await page.getByLabel("Audio source").selectOption("mic-unprocessed");
  await page.getByLabel("Virtual display (WxH/dpi)").fill("1920x1080/240");
  // R-089: camera mirroring reveals facing/size controls, then back to screen.
  await page.getByLabel("Video source").selectOption("camera");
  await page.getByLabel("Camera facing").selectOption("front");
  await page.getByLabel("Camera size (WxH)").fill("1920x1080");
  await page.getByLabel("Video source").selectOption("display");
  await page.getByLabel("Video codec").selectOption("h265");
  await page.getByLabel("Video encoder").selectOption("c2.vendor.hevc.encoder");
  await page.getByRole("checkbox", { name: "Record session" }).check();
  await page
    .getByText(/native save dialog will choose the .mp4 or .mkv/)
    .waitFor();
  await page.getByRole("button", { name: "Launch mirror" }).click();
  await page.getByText("Mirror session running", { exact: true }).waitFor();
  await page
    .getByText(/--record C:\/Users\/QA\/Desktop\/droidsmith-recording/)
    .waitFor();
  await page.getByRole("button", { name: "Stop session" }).click();
  await page.getByText("Mirror session ended", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Launch mirror" }).click();
  await page
    .getByText("Device video encoder failed", { exact: true })
    .waitFor();
  await page.getByText(/MediaCodec encoder failed to initialize/).waitFor();

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByLabel("Language", { exact: true }).selectOption("ru");
  await page.waitForFunction(
    () =>
      document.documentElement.lang === "ru" &&
      document.documentElement.dir === "ltr",
  );

  // IMP-76: a desktop persistence rejection is visible while the selected
  // locale remains active for this session.
  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_FAIL_NEXT__("set_settings_language"),
  );
  await page.getByLabel("Язык", { exact: true }).selectOption("es");
  await page.waitForFunction(() => document.documentElement.lang === "es");
  await page
    .getByRole("alert")
    .getByText(/set_settings_language/)
    .waitFor();
  if (
    (await page.getByLabel("Idioma", { exact: true }).inputValue()) !== "es"
  ) {
    throw new Error("Language persistence failure reverted the active locale");
  }

  // IMP-68: exercise Settings export and reset flows. A subsequent successful
  // save clears the persistence error and returns the smoke flow to English.
  await page.getByLabel("Idioma", { exact: true }).selectOption("en");
  await page.waitForFunction(() => document.documentElement.lang === "en");
  if (await page.getByText(/set_settings_language/).isVisible()) {
    throw new Error("Language persistence error did not clear after a retry");
  }
  await page.getByRole("button", { name: "Export scope" }).click();
  await page.getByText(/Settings exported to/).waitFor();

  // R-104: preview a complete portable import without exposing values, choose
  // replace semantics, apply it, and restore the automatic pre-import backup.
  await page.getByRole("button", { name: "Import settings" }).click();
  await page
    .getByRole("heading", { name: "Review portable settings" })
    .waitFor();
  await page.getByLabel("Import behavior").selectOption("replace");
  await page.getByText("Wireless endpoints").waitFor();
  await page.getByText(/deviceFingerprints/).waitFor();
  await page.getByRole("button", { name: "Apply import" }).click();
  await page.getByText(/A pre-import backup is available/).waitFor();
  await page.getByRole("button", { name: "Restore pre-import backup" }).click();
  await page.getByText(/pre-import settings backup was restored/).waitFor();

  // A malformed/unreadable source is reported without opening a preview or
  // changing current settings.
  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_FAIL_NEXT__("preview_settings_import"),
  );
  await page.getByRole("button", { name: "Import settings" }).click();
  await page.getByText(/preview_settings_import/).waitFor();
  await page.getByRole("button", { name: "Reset scope" }).click();
  await page.getByRole("button", { name: "Reset selected scope" }).click();
  await page.getByText(/reset to defaults/).waitFor();

  // Close the app settings modal opened above so it stops intercepting clicks.
  await page.keyboard.press("Escape");
  await page
    .getByRole("dialog", { name: /settings/i })
    .waitFor({ state: "hidden" });

  // R-082: the Tuning route reads and writes the curated system-settings
  // catalog and keeps a revertable change log.
  await page.getByRole("button", { name: /Tuning/ }).click();
  await page.getByRole("heading", { name: "Tuning", exact: true }).waitFor();
  const animationCard = page
    .getByRole("heading", { name: "Window animation scale", exact: true })
    .locator("../../..");
  await animationCard.waitFor();
  await animationCard
    .getByLabel("Window animation scale", { exact: true })
    .fill("0.5");
  await animationCard.getByRole("button", { name: "Apply" }).click();
  await page
    .getByText("adb shell settings put global window_animation_scale 0.5")
    .first()
    .waitFor();
  await page.getByRole("button", { name: "Revert" }).first().click();
  await page
    .getByText("adb shell settings put global window_animation_scale 1.0")
    .first()
    .waitFor();

  assertNoConsoleErrors(errors, "desktop route smoke");
  await page.close();
}

async function runMobileFlow(browser) {
  const context = await browser.newContext({
    viewport: { width: 390, height: 844 },
    isMobile: true,
  });
  const page = await context.newPage();
  const errors = collectConsoleErrors(page);
  await installTauriMock(page);
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.getByRole("button", { name: "About", exact: true }).click();
  await page.getByRole("button", { name: "Diagnostics", exact: true }).click();
  await page.getByRole("dialog", { name: "Diagnostics center" }).waitFor();
  await assertNoHorizontalOverflow(page, "mobile Diagnostics center");
  await page.getByRole("button", { name: "Close", exact: true }).click();
  await page.getByRole("button", { name: /Apps/ }).click();
  await page.getByText("com.example.app").waitFor();
  await assertNoHorizontalOverflow(page, "mobile Apps route");
  await page.screenshot({
    path: path.join(screenshotDir, "mobile-apps-route.png"),
    fullPage: false,
  });
  assertNoConsoleErrors(errors, "mobile route smoke");
  await context.close();
}

// IMP-51: assert every route renders cleanly under a non-English locale and a
// 200% accessibility zoom, catching layout overflow and locale-only crashes the
// English desktop/mobile flows miss.
async function runLocaleZoomFlow(browser) {
  // A 640px CSS viewport reflows a 1280px desktop at 200% accessibility zoom
  // (WCAG 1.4.10 reflow), the layout an actual 200% zoom produces.
  const page = await browser.newPage({ viewport: { width: 640, height: 900 } });
  const errors = collectConsoleErrors(page);
  await installTauriMock(page);
  await page.goto(baseUrl, { waitUntil: "networkidle" });

  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByLabel("Language", { exact: true }).selectOption("ru");
  await page.waitForFunction(() => document.documentElement.lang === "ru");
  await page.keyboard.press("Escape");

  const routes = [
    "Устройства",
    "Беспроводное",
    "Приложения",
    "Очистка",
    "Профили",
    "Зеркало",
    "Консоль",
    "Logcat",
    "Fastboot",
    "Настройка",
  ];
  for (const name of routes) {
    await page
      .getByRole("button", { name: new RegExp(name, "u") })
      .first()
      .click();
    await page.getByRole("heading", { name, exact: true }).first().waitFor();
    await assertNoDocumentOverflow(page, `ru+zoom ${name}`);
  }

  await page.screenshot({
    path: path.join(screenshotDir, "locale-zoom-logcat.png"),
    fullPage: false,
  });
  assertNoConsoleErrors(errors, "locale+zoom route smoke");
  await page.close();
}

// IMP-51: exercise the loading, error, and empty rendered states plus a
// stale-completion race and a detached-workspace guard that the golden-path
// desktop flow never reaches.
async function runResilienceFlow(browser) {
  const page = await browser.newPage({
    viewport: { width: 1366, height: 900 },
  });
  const errors = collectConsoleErrors(page);
  await installTauriMock(page);
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.getByRole("button", { name: /Apps/ }).waitFor();

  // Loading state: hold the package listing so the skeleton renders, then
  // release it and confirm the real content replaces it.
  await page.evaluate(() => window.__DROIDSMITH_MOCK_GATE__("list_packages"));
  await page.getByRole("button", { name: /Apps/ }).click();
  await page.locator('[aria-label="Loading packages"]').waitFor();
  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_RELEASE__("list_packages"),
  );
  await page.getByText("com.example.app").waitFor();

  // Empty state: an authorized device with zero packages must show the empty
  // panel, not a bare table.
  await page.evaluate(() => window.__DROIDSMITH_MOCK_EMPTY_PACKAGES__(true));
  await page.getByRole("button", { name: "Refresh packages" }).click();
  await page.getByText("No matching packages").waitFor();
  await page.evaluate(() => window.__DROIDSMITH_MOCK_EMPTY_PACKAGES__(false));

  // Error state: a failed enumeration surfaces a danger panel and recovers on
  // the next successful refresh.
  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_FAIL_NEXT__("list_packages"),
  );
  await page.getByRole("button", { name: "Refresh packages" }).click();
  await page.getByText("Package enumeration failed").waitFor();
  await page.getByRole("button", { name: "Refresh packages" }).click();
  await page.getByText("com.example.app").waitFor();

  // Stale completion: hold a package listing, bump the device generation via a
  // hotplug reconnect while it is in flight, then release the stale response.
  // The workspace must reflect the reconnect, not the superseded request, and
  // must not raise a console error.
  await page.evaluate(() => window.__DROIDSMITH_MOCK_GATE__("list_packages"));
  await page.getByRole("button", { name: "Refresh packages" }).click();
  await page.locator('[aria-label="Loading packages"]').waitFor();
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(false));
  await page.getByText("No authorized devices", { exact: true }).waitFor();
  // A destructive control cannot be present once the workspace is detached.
  if (
    (await page.getByRole("button", { name: "Refresh packages" }).count()) !== 0
  ) {
    throw new Error("Detached Apps workspace still exposes package controls");
  }
  await page.evaluate(() =>
    window.__DROIDSMITH_MOCK_RELEASE__("list_packages"),
  );
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(true));
  await page.getByText("com.example.app").waitFor();

  await page.screenshot({
    path: path.join(screenshotDir, "resilience-apps-states.png"),
    fullPage: false,
  });
  assertNoConsoleErrors(errors, "resilience route smoke");
  await page.close();
}

async function runWatcherFallbackFlow(browser) {
  const page = await browser.newPage({
    viewport: { width: 1366, height: 900 },
  });
  const errors = collectConsoleErrors(page);
  await installTauriMock(page, { failAlwaysCommands: ["watch_devices"] });
  await page.goto(baseUrl, { waitUntil: "networkidle" });

  await page.getByText("ADB ready", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Select Pixel QA" }).waitFor();
  await page.getByText("Watcher stopped", { exact: true }).waitFor();
  assertNoConsoleErrors(errors, "watcher fallback smoke");
  await page.close();
}

// Render the README's published routes from mocked-native state, assert none of
// them show the browser-only "desktop shell required" placeholder, and — when
// invoked with --capture-docs — refresh the committed screenshots. Also renders
// each at a narrow width so the documented workflows are verified at both sizes.
async function runDocCaptureFlow(browser) {
  const shots = [
    {
      name: "droidsmith-overview",
      nav: /Devices/,
      anchor: (page) => page.getByText("ADB lifecycle health", { exact: true }),
    },
    {
      name: "droidsmith-apps",
      nav: /Apps/,
      anchor: (page) => page.getByText("com.example.app").first(),
    },
    {
      name: "droidsmith-mirror",
      nav: /Mirror/,
      anchor: (page) => page.getByText("scrcpy 4.0", { exact: true }),
    },
  ];

  if (captureDocs) fs.mkdirSync(docsScreenshotDir, { recursive: true });

  const page = await browser.newPage({
    viewport: { width: 1366, height: 900 },
  });
  const errors = collectConsoleErrors(page);
  await installTauriMock(page);
  await page.goto(baseUrl, { waitUntil: "networkidle" });

  for (const shot of shots) {
    await page.getByRole("button", { name: shot.nav }).first().click();
    await shot.anchor(page).waitFor();
    if (
      (await page
        .getByText("Desktop shell required", { exact: true })
        .count()) !== 0
    ) {
      throw new Error(
        `${shot.name} still renders the desktop-required placeholder`,
      );
    }
    if (captureDocs) {
      await page.screenshot({
        path: path.join(docsScreenshotDir, `${shot.name}.png`),
        fullPage: false,
      });
    }
  }

  // The documented workflows must also survive a narrow viewport.
  await page.setViewportSize({ width: 390, height: 844 });
  for (const shot of shots) {
    await page.getByRole("button", { name: shot.nav }).first().click();
    await shot.anchor(page).waitFor();
    await assertNoHorizontalOverflow(page, `narrow ${shot.name}`);
  }

  assertNoConsoleErrors(errors, "doc capture smoke");
  await page.close();
}

function collectConsoleErrors(page) {
  const errors = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(msg.text());
    }
  });
  page.on("pageerror", (error) => errors.push(error.message));
  return errors;
}

function assertNoConsoleErrors(errors, label) {
  if (errors.length > 0) {
    throw new Error(`${label} console errors:\n${errors.join("\n")}`);
  }
}

async function assertFocusedLabel(page, label) {
  await page.waitForFunction(
    (expected) =>
      document.activeElement?.getAttribute("aria-label") === expected,
    label,
  );
  const activeLabel = await page.evaluate(() => {
    const active = document.activeElement;
    if (!active) return "";
    return active.getAttribute("aria-label") ?? "";
  });
  if (activeLabel !== label) {
    throw new Error(`Expected focus on ${label}, got ${activeLabel}`);
  }
}

async function assertTabMovesFocus(page, label) {
  await page.keyboard.press("Tab");
  const focused = await page.evaluate(() => {
    const active = document.activeElement;
    if (!active || active === document.body) return "";
    return `${active.tagName}:${active.textContent?.trim() ?? ""}`;
  });
  if (!focused) {
    throw new Error(`${label} did not expose a tabbable control`);
  }
}

async function assertMainFocused(page, route) {
  try {
    await page.waitForFunction(
      (expected) => {
        const active = document.activeElement;
        return (
          active?.tagName === "MAIN" &&
          active.getAttribute("aria-label") === expected
        );
      },
      route,
      { timeout: 3_000 },
    );
  } catch {
    const focusState = await page.evaluate(() => ({
      activeTag: document.activeElement?.tagName,
      activeLabel: document.activeElement?.getAttribute("aria-label"),
      activeText: document.activeElement?.textContent?.trim().slice(0, 80),
      mainLabel: document.querySelector("main")?.getAttribute("aria-label"),
      paletteDialogs: document.querySelectorAll(
        "[role='dialog'][aria-labelledby='command-palette-title']",
      ).length,
      activeConnected: document.activeElement?.isConnected,
    }));
    throw new Error(
      `${route} route did not move focus to main: ${JSON.stringify(focusState)}`,
    );
  }
}

async function assertFocusedButton(page, name) {
  await page.waitForFunction((expected) => {
    const active = document.activeElement;
    return (
      active?.getAttribute("role") === null &&
      active?.tagName === "BUTTON" &&
      active.textContent?.trim() === expected
    );
  }, name);
}

// The WCAG 1.4.10 reflow criterion: content must not require two-dimensional
// scrolling. This checks the document itself rather than per-element scrollWidth
// (native selects clip long option text and ellipsis text legitimately overflow
// their own box without widening the page).
async function assertNoDocumentOverflow(page, label) {
  await settleLayout(page);
  const overflow = await page.evaluate(() => {
    const root = document.documentElement;
    return root.scrollWidth - root.clientWidth;
  });
  if (overflow > 2) {
    throw new Error(`${label} document overflow: ${overflow}px`);
  }
}

async function settleLayout(page) {
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        const settle = () =>
          requestAnimationFrame(() => requestAnimationFrame(resolve));
        if (document.fonts && document.fonts.ready) {
          void document.fonts.ready.then(settle);
        } else {
          settle();
        }
      }),
  );
}

async function assertNoHorizontalOverflow(page, label) {
  // Let web-font metrics and layout settle before measuring so a mid-reflow
  // paragraph (e.g. under a locale switch or zoom change) cannot report a
  // transient overflow.
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        const settle = () =>
          requestAnimationFrame(() => requestAnimationFrame(resolve));
        if (document.fonts && document.fonts.ready) {
          void document.fonts.ready.then(settle);
        } else {
          settle();
        }
      }),
  );
  const offenders = await page
    .locator(
      "button, select, input, label, h1, h2, h3, h4, p, [role='option'], [role='radio']",
    )
    .evaluateAll((elements) =>
      elements.flatMap((element) => {
        if (!(element instanceof HTMLElement)) return [];
        if (element.closest(".overflow-x-auto")) return [];
        if (element.closest(".sr-only")) return [];
        const rect = element.getBoundingClientRect();
        if (rect.width < 1 || rect.height < 1) return [];
        const style = getComputedStyle(element);
        if (style.display === "none" || style.visibility === "hidden")
          return [];
        if (element.scrollWidth <= element.clientWidth + 2) return [];
        const text = (element.textContent ?? "").trim().replace(/\s+/g, " ");
        return [`${element.tagName.toLowerCase()}: ${text.slice(0, 120)}`];
      }),
    );

  if (offenders.length > 0) {
    throw new Error(`${label} horizontal overflow:\n${offenders.join("\n")}`);
  }
}

async function installTauriMock(
  page,
  { failStartupCommands = [], failAlwaysCommands = [] } = {},
) {
  await page.addInitScript(
    (options) => {
      window.__DROIDSMITH_MOCK_OPTIONS__ = options;
    },
    { failStartupCommands, failAlwaysCommands },
  );
  await page.addInitScript(() => {
    const { failStartupCommands, failAlwaysCommands } =
      window.__DROIDSMITH_MOCK_OPTIONS__;
    const callbacks = new Map();
    const channelIndexes = new Map();
    const pendingOperations = new Map();
    let nextCallbackId = 1;
    let nextEventId = 1;
    const device = {
      serial: "QA123",
      state: "device",
      model: "Pixel QA",
      product: "oriole",
      device: "oriole",
      build_fingerprint: "google/oriole/oriole:15/QA",
      transport_id: 7,
      connection_generation: 8,
      transport_kind: "usb",
      wireless: false,
    };
    const platformToolsPolicy = {
      status: "supported",
      rationale:
        "Platform Tools 37.0.0 makes libadbmdns the default mDNS backend.",
      recommended_version: "37.0.0",
      warning_below_version: "36.0.2",
      policy_reviewed_on: "2026-07-15",
      source_url: "https://developer.android.com/tools/releases/platform-tools",
    };
    const adbHealth = {
      server_status_supported: true,
      client_version: "37.0.0",
      server_version: "37.0.0",
      server_build: "123456",
      usb_backend: "NATIVE",
      mdns_backend: "LIBADBMDNS",
      mdns_enabled: true,
      mdns_check: "mDNS responder available",
      burst_mode: true,
      recommended_for_wifi_v2: true,
      wifi_v2_state: "supported",
      wifi_v2_devices: ["Pixel QA"],
      warning: null,
      platform_tools: platformToolsPolicy,
    };
    const packages = [
      {
        package: "com.example.app",
        enabled: true,
        archived: false,
        system: false,
        apk_path: "/data/app/com.example.app/base.apk",
        uid: 10101,
        installer: "com.android.vending",
      },
      {
        package: "com.android.settings",
        enabled: true,
        archived: false,
        system: true,
        apk_path: "/system/priv-app/Settings/Settings.apk",
        uid: 1000,
        installer: null,
      },
      {
        package: "com.example.disabled",
        enabled: false,
        archived: false,
        system: false,
        apk_path: "/data/app/com.example.disabled/base.apk",
        uid: 10102,
        installer: null,
      },
      {
        package: "com.example.fail",
        enabled: true,
        archived: false,
        system: false,
        apk_path: "/data/app/com.example.fail/base.apk",
        uid: 10103,
        installer: null,
      },
    ];
    let remoteFiles = [
      {
        name: "Résumé final.txt",
        is_dir: false,
        size: 2048,
        permissions: "-rw-rw----",
        parse_error: null,
      },
      {
        name: "Protected notes.txt",
        is_dir: false,
        size: 1024,
        permissions: "-r--------",
        parse_error: null,
      },
    ];
    let journalId = 20;
    const runtimeJournal = [];
    let installAttempts = 0;
    let scrcpyLaunches = 0;
    let fingerprintObserved = false;
    let gnirehtetStopped = false;
    let settingsLanguage = "en";
    let settingsImportBackupAvailable = false;
    const logcatQueries = { global: [] };
    let archiveApi = 35;
    window.__DROIDSMITH_MOCK_ARCHIVE_API__ = (api) => {
      archiveApi = api;
    };

    // IMP-51 resilience controls: force the next call of a command to reject,
    // hold a command's response until released (to reproduce loading and stale
    // completions), and empty a listing to exercise empty states.
    const failNext = new Set(failStartupCommands);
    const failAlways = new Set(failAlwaysCommands);
    const gateResolvers = new Map();
    const gatePromises = new Map();
    let emptyPackages = false;
    window.__DROIDSMITH_MOCK_FAIL_NEXT__ = (cmd) => failNext.add(cmd);
    window.__DROIDSMITH_MOCK_GATE__ = (cmd) => {
      gatePromises.set(
        cmd,
        new Promise((resolve) => gateResolvers.set(cmd, resolve)),
      );
    };
    window.__DROIDSMITH_MOCK_RELEASE__ = (cmd) => {
      const resolve = gateResolvers.get(cmd);
      if (resolve) {
        resolve();
        gateResolvers.delete(cmd);
      }
    };
    window.__DROIDSMITH_MOCK_EMPTY_PACKAGES__ = (value) => {
      emptyPackages = value;
    };
    // R-082 tuning: curated device-settings catalog with mutable values so
    // put_device_setting round-trips through the subsequent list.
    const deviceSettingsCatalog = [
      {
        id: "window_animation_scale",
        namespace: "global",
        key: "window_animation_scale",
        control: { kind: "float", min: 0, max: 10 },
        value: "1.0",
      },
      {
        id: "transition_animation_scale",
        namespace: "global",
        key: "transition_animation_scale",
        control: { kind: "float", min: 0, max: 10 },
        value: "1.0",
      },
      {
        id: "animator_duration_scale",
        namespace: "global",
        key: "animator_duration_scale",
        control: { kind: "float", min: 0, max: 10 },
        value: "1.0",
      },
      {
        id: "screen_off_timeout",
        namespace: "system",
        key: "screen_off_timeout",
        control: { kind: "int", min: 15000, max: 1800000 },
        value: "30000",
      },
      {
        id: "font_scale",
        namespace: "system",
        key: "font_scale",
        control: { kind: "float", min: 0.5, max: 2 },
        value: null,
      },
      {
        id: "stay_on_while_plugged_in",
        namespace: "global",
        key: "stay_on_while_plugged_in",
        control: {
          kind: "choice",
          options: [
            { value: "0", label: "Off" },
            { value: "7", label: "On (any charger)" },
          ],
        },
        value: "0",
      },
    ];
    const qaPackAssessment = {
      status: "compatible",
      override_required: false,
      checks: [
        {
          field: "manufacturer",
          status: "compatible",
          expected: ["Google"],
          actual: "Google",
        },
        {
          field: "build_fingerprint",
          status: "compatible",
          expected: ["google/"],
          actual: "google/oriole/oriole:15/QA",
        },
        {
          field: "android_user",
          status: "compatible",
          expected: ["owner"],
          actual: "0 (current)",
        },
      ],
      entries: [
        { id: "com.example.app", status: "ready", detail: null },
        { id: "com.example.fail", status: "ready", detail: null },
        { id: "com.android.settings", status: "ready", detail: null },
      ],
    };

    window.__TAURI_INTERNALS__ = {
      async invoke(cmd, args = {}) {
        if (failAlways.has(cmd)) {
          throw new Error(`mock-forced persistent failure: ${cmd}`);
        }
        if (failNext.has(cmd)) {
          failNext.delete(cmd);
          throw new Error(`mock-forced failure: ${cmd}`);
        }
        if (gatePromises.has(cmd)) {
          const pending = gatePromises.get(cmd);
          gatePromises.delete(cmd);
          await pending;
        }
        // Tauri event-plugin plumbing used by the Apps drag-drop listener
        // (getCurrentWebview().onDragDropEvent). The smoke harness never emits
        // a real drop, so registration just needs to resolve without throwing.
        if (cmd === "plugin:event|listen") {
          return nextEventId++;
        }
        if (cmd === "plugin:event|unlisten") {
          return null;
        }
        if (cmd === "select_host_path") {
          const selections = {
            diagnostics_save: {
              id: "123e4567-e89b-42d3-a456-426614174001",
              local_path: "C:/Users/QA/Desktop/droidsmith-support.json",
            },
            bugreport_save: {
              id: "123e4567-e89b-42d3-a456-426614174008",
              local_path:
                "C:/Users/QA/Desktop/droidsmith-bugreport-2026-07-15.zip",
            },
            scrcpy_record_save: {
              id: "123e4567-e89b-42d3-a456-426614174009",
              local_path:
                "C:/Users/QA/Desktop/droidsmith-recording-2026-07-15.mp4",
            },
            install_open: {
              id: "123e4567-e89b-42d3-a456-426614174002",
              local_path: "C:/Users/QA/Downloads/sample.apks",
            },
            recovery_baseline_save: {
              id: "123e4567-e89b-42d3-a456-426614174004",
              local_path:
                "C:/Users/QA/Desktop/droidsmith-recovery-2026-07-15-qa-debloat.json",
            },
            recovery_baseline_open: {
              id: "123e4567-e89b-42d3-a456-426614174005",
              local_path: "C:/Users/QA/Desktop/imported-recovery.json",
            },
            profile_save: {
              id: "123e4567-e89b-42d3-a456-42661417400a",
              local_path: "C:/Users/QA/Desktop/qa-profile-v2.yaml",
            },
            profile_open: {
              id: "123e4567-e89b-42d3-a456-42661417400b",
              local_path: "C:/Users/QA/Desktop/legacy-profile-v1.yaml",
            },
            package_export_save: {
              id: "123e4567-e89b-42d3-a456-426614174006",
              local_path: "C:/Users/QA/Desktop/com.example.app.apks.zip",
            },
            backup_save: {
              id: "123e4567-e89b-42d3-a456-426614174007",
              local_path: "C:/Users/QA/Desktop/com.example.app.legacy-data.zip",
            },
            push_open: {
              id: "123e4567-e89b-42d3-a456-42661417400c",
              local_path: "C:/Users/QA/Downloads/新しい report.txt",
            },
            layout_export_save: {
              id: "123e4567-e89b-42d3-a456-42661417400d",
              local_path: "C:/Users/QA/Desktop/layout-QA123.xml",
            },
            settings_export: {
              id: "123e4567-e89b-42d3-a456-42661417400e",
              local_path: "C:/Users/QA/Desktop/droidsmith-settings-all.json",
            },
            settings_import: {
              id: "123e4567-e89b-42d3-a456-426614174011",
              local_path: "C:/Users/QA/Desktop/imported-settings.json",
            },
            apk_analyze_open: {
              id: "123e4567-e89b-42d3-a456-42661417400f",
              local_path: "C:/Users/QA/Downloads/sample.apk",
            },
            pack_export_save: {
              id: "123e4567-e89b-42d3-a456-426614174010",
              local_path: "C:/Users/QA/Desktop/device-debloat.yaml",
            },
          };
          const selection = selections[args.purpose];
          if (!selection) {
            throw new Error(
              `Unhandled mocked host path purpose: ${args.purpose}`,
            );
          }
          return { ...selection, purpose: args.purpose };
        }
        if (cmd === "heartbeat") {
          return {
            version: "0.1.0",
            os: { family: "Windows", version: "11", arch: "x86_64" },
            tauri_version: "2.0.0",
            rust_version: "1.88.0",
            app_data_dir: "C:/Users/QA/AppData/Roaming/Droidsmith",
            adb: {
              path: "C:/Android/platform-tools/adb.exe",
              source: "path",
              version: "35.0.2",
              compatibility: {
                ...platformToolsPolicy,
                status: "warn",
                rationale:
                  "Platform Tools 35.0.2 predates the 36.0.2 reliability floor.",
              },
            },
          };
        }
        if (cmd === "run_host_doctor") {
          return {
            scanned_at: "2026-07-15T10:00:00Z",
            platform: "windows",
            adb: {
              resolved: true,
              source: "path",
              version: "37.0.0",
              query_succeeded: true,
              client_version: "37.0.0",
              server_version: "37.0.0",
              compatibility: platformToolsPolicy,
            },
            device_state_counts: { device: 1 },
            findings: [
              {
                code: "adb_ready",
                severity: "info",
                title: "ADB executable is ready",
                summary:
                  "Droidsmith resolved ADB and completed transport enumeration.",
                evidence: ["Platform Tools version: 37.0.0"],
                remediation: ["Keep Platform Tools current."],
                official_url:
                  "https://developer.android.com/tools/releases/platform-tools",
              },
              {
                code: "server_config_override",
                severity: "warning",
                title: "Custom ADB server configuration is active",
                summary:
                  "An environment variable overrides the default ADB server.",
                evidence: ["ADB_SERVER_SOCKET is set (value redacted)"],
                remediation: ["Review the named environment variable."],
                official_url: "https://developer.android.com/tools/adb",
              },
            ],
            privacy: [
              "No changes were made.",
              "Device serials and environment values were not retained.",
            ],
          };
        }
        if (cmd === "list_devices") {
          return {
            adb_resolved: true,
            adb_path: "C:/Android/platform-tools/adb.exe",
            devices: [device],
          };
        }
        if (cmd === "list_wireless_services") {
          return {
            adb_resolved: true,
            adb_path: "C:/Android/platform-tools/adb.exe",
            services: [],
          };
        }
        if (cmd === "connect_wireless") {
          throw {
            code: "wireless_adb_failed",
            message: "adb exited with code 1: failed to connect",
            hint_code: "vpn_interference_likely",
            diagnostics: {
              platform_tools_version: "37.0.0",
              mdns_enabled: true,
              mdns_backend: "LIBADBMDNS",
              mdns_check_succeeded: true,
              active_vpn_interfaces: 1,
              endpoint_kind: "local_name",
              adb_error_kind: "adb_exit",
            },
          };
        }
        if (cmd === "list_wireless_history") {
          return {
            endpoints: [
              {
                host: "192.168.1.42",
                port: 5555,
                label: null,
                lastConnectedMs: 1_752_000_000_000,
              },
            ],
            autoReconnect: false,
          };
        }
        if (cmd === "forget_wireless_endpoint") {
          return { endpoints: [], autoReconnect: false };
        }
        if (cmd === "set_wireless_auto_reconnect") {
          return {
            endpoints: [
              {
                host: "192.168.1.42",
                port: 5555,
                label: null,
                lastConnectedMs: 1_752_000_000_000,
              },
            ],
            autoReconnect: Boolean(args.enabled),
          };
        }
        if (cmd === "watch_devices") {
          window.__DROIDSMITH_MOCK_HOTPLUG__ = (connected) => {
            if (connected) {
              device.transport_id += 1;
              device.connection_generation += 1;
            }
            emitChannel(args.on_event, {
              kind: "snapshot",
              result: {
                adb_resolved: true,
                adb_path: "C:/Android/platform-tools/adb.exe",
                devices: connected ? [{ ...device }] : [],
              },
              health: adbHealth,
              observed_at: "2026-07-14T18:00:03Z",
            });
          };
          window.__DROIDSMITH_MOCK_TRANSPORT__ = (kind) => {
            device.transport_kind = kind;
            device.wireless = kind !== "usb";
            device.connection_generation += 1;
            emitChannel(args.on_event, {
              kind: "snapshot",
              result: {
                adb_resolved: true,
                adb_path: "C:/Android/platform-tools/adb.exe",
                devices: [{ ...device }],
              },
              health: adbHealth,
              observed_at: "2026-07-14T18:00:04Z",
            });
          };
          emitChannel(args.on_event, {
            kind: "snapshot",
            result: {
              adb_resolved: true,
              adb_path: "C:/Android/platform-tools/adb.exe",
              devices: [device],
            },
            health: adbHealth,
            observed_at: "2026-07-14T18:00:00Z",
          });
          return new Promise((resolve) => {
            pendingOperations.set(args.operation_id, {
              resolve,
              channel: args.on_event,
            });
          });
        }
        if (cmd === "recover_adb") {
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "started",
            message: "Recovery sequence started",
          });
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "progress",
            message: "Step 3/3: adb reconnect offline",
          });
          return {
            record_path:
              "C:/Users/QA/AppData/Roaming/Droidsmith/host-operations.jsonl",
            record: {
              schema_version: 1,
              operation_id: args.operation_id,
              operation: "adb_server_recovery",
              confirmation_source: "devices_health_review",
              outcome: "succeeded",
              started_at: "2026-07-14T18:00:00Z",
              completed_at: "2026-07-14T18:00:02Z",
              commands: [
                ["kill-server"],
                ["start-server"],
                ["reconnect", "offline"],
              ],
              health_before: adbHealth,
              health_after: adbHealth,
              failure: null,
            },
          };
        }
        if (cmd === "preview_diagnostics") {
          const content = JSON.stringify(
            {
              schema_version: 1,
              generated_at: "2026-07-14T18:02:00Z",
              privacy: {
                local_only: true,
                uploads_performed: false,
                redactions: [
                  "raw device serials",
                  "network device addresses",
                  "wireless pairing secrets",
                  "host filesystem paths",
                  "credential-like values",
                ],
              },
              environment: {
                app_version: "0.1.0",
                adb_version: "37.0.0",
                adb_compatibility: platformToolsPolicy,
              },
              devices: [
                { id: "device-01", state: "device", model: "Pixel QA" },
              ],
              failed_operations: [
                {
                  source: "device_journal",
                  device_id: "device-01",
                  operation_id: "op-redacted",
                  operation: "disable",
                  outcome: "failed",
                },
              ],
              crash_logs: [],
            },
            null,
            2,
          );
          return {
            generated_at: "2026-07-14T18:02:00Z",
            content,
            byte_size: content.length,
            device_count: 1,
            failed_operation_count: 1,
            crash_line_count: 0,
            local_only: true,
          };
        }
        if (cmd === "save_diagnostics") {
          if (args.path_grant !== "123e4567-e89b-42d3-a456-426614174001") {
            throw new Error("Diagnostics save did not consume its path grant");
          }
          return {
            path: "C:/Users/QA/Desktop/droidsmith-support.json",
            byte_size: 1024,
            generated_at: "2026-07-14T18:02:01Z",
          };
        }
        if (cmd === "capture_bugreport") {
          if (
            args.path_grant !== "123e4567-e89b-42d3-a456-426614174008" ||
            args.privacy_confirmed !== true ||
            args.target.transport_id !== 7
          ) {
            throw new Error(
              "Bugreport capture did not preserve its grant, consent, and immutable target",
            );
          }
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "started",
            message: "Capturing sensitive Android bugreport",
          });
          return {
            report: {
              local_path:
                "C:/Users/QA/Desktop/droidsmith-bugreport-2026-07-15.zip",
              size_bytes: 12_582_912,
              sha256: "a".repeat(64),
            },
            sidecar: {
              local_path:
                "C:/Users/QA/Desktop/droidsmith-bugreport-2026-07-15.zip.metadata.json",
              size_bytes: 512,
              sha256: "b".repeat(64),
            },
            captured_at: "2026-07-15T10:15:00Z",
          };
        }
        if (cmd === "wipe_diagnostics") {
          return {
            files_removed: 2,
            bytes_removed: 4096,
            device_journals_preserved: true,
          };
        }
        if (cmd === "list_device_settings") {
          return deviceSettingsCatalog.map((setting) => ({ ...setting }));
        }
        if (cmd === "put_device_setting") {
          const setting = deviceSettingsCatalog.find(
            (item) => item.id === args.settingId,
          );
          if (!setting) {
            throw new Error(`unknown setting: ${args.settingId}`);
          }
          const previous = setting.value;
          setting.value = String(args.value);
          return {
            id: setting.id,
            namespace: setting.namespace,
            key: setting.key,
            previous_value: previous,
            new_value: setting.value,
            command: `adb shell settings put ${setting.namespace} ${setting.key} ${setting.value}`,
          };
        }
        if (cmd === "list_packages") {
          return {
            packages: emptyPackages
              ? []
              : filterPackages(packages, args.filter ?? "all"),
            archive: {
              supported: archiveApi >= 35,
              api_level: archiveApi,
              reason:
                archiveApi >= 35
                  ? "Android 15+ package archiving is available"
                  : `package archiving requires Android 15 (API 35); device reports API ${archiveApi}`,
            },
          };
        }
        if (cmd === "observe_device_fingerprint") {
          // Model an OTA update once, then record it: the first observation
          // reports changed, subsequent ones do not (matching the backend).
          if (fingerprintObserved) {
            return { changed: false, previous: "acme/qa/2:14/NEW" };
          }
          fingerprintObserved = true;
          return { changed: true, previous: "acme/qa/1:13/OLD" };
        }
        if (cmd === "get_device_info") {
          return {
            serial: args.target.serial,
            model: "Pixel QA",
            manufacturer: "Google",
            android_version: "17",
            sdk_level: "36",
            build_fingerprint: args.target.build_fingerprint,
            security_patch: "2026-07-01",
            hardware_serial: null,
            battery: {
              level: 82,
              status: "Discharging",
              temperature: 30.5,
              health: "Good",
              voltage_mv: 4187,
              technology: "Li-ion",
              cycle_count: 148,
              charge_counter_uah: 3450000,
            },
            storage: {
              total_kb: 113021876,
              used_kb: 87654320,
              available_kb: 25367556,
            },
            storage_partitions: [
              {
                mount: "/system",
                total_kb: 2500000,
                used_kb: 2400000,
                available_kb: 100000,
              },
              {
                mount: "/data",
                total_kb: 113021876,
                used_kb: 87654320,
                available_kb: 25367556,
              },
            ],
            thermal_zones: [
              { name: "CPU", temperature_c: 31.4, status: "None" },
              { name: "battery", temperature_c: 30.5, status: "Light" },
            ],
            wifi_ip: null,
          };
        }
        if (cmd === "list_remote_files") {
          return {
            path: args.remote_path,
            entries: remoteFiles.map((entry) => ({ ...entry })),
            free_space_kb: 8_388_608,
          };
        }
        if (cmd === "plan_remote_file_mutation") {
          return remoteFilePlan(args.request);
        }
        if (cmd === "apply_remote_file_mutation") {
          if (args.confirmed !== true) {
            throw new Error("confirmation_required");
          }
          const plan = remoteFilePlan(args.request);
          if (plan.source_path.includes("Protected notes.txt")) {
            throw new Error(
              "remote_file_operation_failed: Permission denied by Android storage policy",
            );
          }
          const sourceName = plan.source_path.split("/").pop();
          if (plan.kind === "mkdir") {
            remoteFiles.push({
              name: sourceName,
              is_dir: true,
              size: null,
              permissions: "drwxrwx---",
              parse_error: null,
            });
          } else if (plan.kind === "rename") {
            const entry = remoteFiles.find((item) => item.name === sourceName);
            if (entry) entry.name = plan.destination_path.split("/").pop();
          } else {
            remoteFiles = remoteFiles.filter(
              (item) => item.name !== sourceName,
            );
          }
          return fileMutationResult(plan);
        }
        if (cmd === "push_file") {
          if (
            args.confirmed !== true ||
            args.path_grant !== "123e4567-e89b-42d3-a456-42661417400c"
          ) {
            throw new Error(
              "File push bypassed its confirmation or path grant",
            );
          }
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "progress",
            message: "Pushing file to device",
            elapsed_ms: 1200,
          });
          remoteFiles.push({
            name: args.remote_path.split("/").pop(),
            is_dir: false,
            size: 4096,
            permissions: "-rw-rw----",
            parse_error: null,
          });
          return fileMutationResult({
            kind: "shell",
            source_path: args.remote_path,
            destination_path: null,
            argv: ["droidsmith-file-push", args.remote_path],
            description: "Push native-selected file",
            destructive: false,
          });
        }
        if (cmd === "get_package_metadata") {
          return {
            package: args.package,
            label: args.package === "com.example.app" ? "Example App" : null,
            icon_data_uri: null,
            cache_hit: false,
          };
        }
        if (cmd === "preflight_package_backup") {
          return {
            package: args.package,
            android_user: args.userId,
            default_capability: "apk_export",
            legacy_capability: "legacy_data_eligible",
            apk_paths: [
              "/data/app/com.example/base.apk",
              "/data/app/com.example/split_config.en.apk",
            ],
            evidence: {
              device_sdk: 35,
              target_sdk: 30,
              debuggable: false,
              allow_backup: true,
              reason:
                "Package targets an API below the Android 12 exclusion threshold.",
            },
          };
        }
        if (cmd === "export_package_apks") {
          if (args.path_grant !== "123e4567-e89b-42d3-a456-426614174006") {
            throw new Error("APK export did not consume its scoped path grant");
          }
          return packageExportResult(
            "apk_export",
            "C:/Users/QA/Desktop/com.example.app.apks.zip",
            null,
            2,
          );
        }
        if (cmd === "backup_package") {
          if (args.path_grant !== "123e4567-e89b-42d3-a456-426614174007") {
            throw new Error(
              "Legacy export did not consume its scoped path grant",
            );
          }
          return packageExportResult(
            "legacy_data",
            "C:/Users/QA/Desktop/com.example.app.legacy-data.zip",
            "app_data_entries_detected",
            1,
          );
        }
        if (cmd === "export_recovery_baseline") {
          if (args.path_grant !== "123e4567-e89b-42d3-a456-426614174004") {
            throw new Error("Recovery export did not consume its path grant");
          }
          return {
            local_path:
              "C:/Users/QA/Desktop/droidsmith-recovery-2026-07-15-qa-debloat.json",
            size_bytes: 2048,
            sha256: "a".repeat(64),
          };
        }
        if (cmd === "inspect_recovery_baseline") {
          const recoveryPlan = planFor({
            serial: "QA123",
            target: args.target,
            package: "com.example.app",
            kind: "enable",
            user_id: 0,
            pack_context: null,
            context: {
              confirmation_source: "recovery_baseline",
              permission: null,
              shell_argv: [],
              transport_override: null,
            },
          });
          return {
            baseline: {
              format: "droidsmith_recovery_baseline",
              schema_version: 1,
              exported_at: "2026-07-01T12:00:00Z",
              device: {
                identity_sha256: "b".repeat(64),
                build_fingerprint: "google/oriole/oriole:14/OLD",
              },
              android_user: 0,
              pack: { id: "qa-debloat", revision: 2 },
              packages: [],
            },
            compatibility: {
              device_identity_matches: true,
              build_fingerprint_matches: false,
              android_user_available: true,
              current_device_identity_sha256: "b".repeat(64),
              current_build_fingerprint: device.build_fingerprint,
            },
            rows: [
              {
                package: "com.example.app",
                baseline_present: true,
                baseline_enabled: true,
                live_present: true,
                live_enabled: false,
                requested_action: "disable",
                status: "ready",
                reason_code: null,
                reason: "review this canonical enable-state recovery action",
              },
              {
                package: "com.example.removed",
                baseline_present: true,
                baseline_enabled: true,
                live_present: false,
                live_enabled: null,
                requested_action: "disable",
                status: "skipped",
                reason_code: "live_package_absent",
                reason: "package is absent from the live Android user",
              },
            ],
            plans: [recoveryPlan],
          };
        }
        if (cmd === "save_profile") {
          if (
            args.path_grant !== "123e4567-e89b-42d3-a456-42661417400a" ||
            args.profile.version !== "2" ||
            args.profile.actions.length === 0
          ) {
            throw new Error("Profile export did not validate schema v2");
          }
          return {
            local_path: "C:/Users/QA/Desktop/qa-profile-v2.yaml",
            size_bytes: 768,
            sha256: "c".repeat(64),
          };
        }
        if (cmd === "inspect_profile") {
          if (args.path_grant !== "123e4567-e89b-42d3-a456-42661417400b") {
            throw new Error("Profile import did not consume its read grant");
          }
          const migrated = {
            name: "Legacy QA setup",
            version: "2",
            description: "Migrated rendered-smoke profile",
            device: {
              require_serial_prefix: "QA",
              require_manufacturer: "Google",
              require_model: "Pixel QA",
              require_android_min: 34,
              require_android_max: 36,
            },
            user: { mode: "owner", id: null },
            actions: [
              { kind: "disable", package: "com.example.app", note: "" },
              {
                kind: "disable",
                package: "com.example.disabled",
                note: "",
              },
            ],
          };
          return {
            source_version: "1",
            profile: migrated,
            migration: {
              from_version: "1",
              to_version: "2",
              profile: migrated,
              warnings: [
                "Android user 0 was promoted to the profile-level owner constraint.",
              ],
            },
            compatible: true,
            compatibility_issues: [],
            android_user: 0,
            rows: [
              {
                action: migrated.actions[0],
                plan: planFor({
                  serial: "QA123",
                  target: args.target,
                  package: "com.example.app",
                  kind: "disable",
                  user_id: 0,
                  context: {
                    confirmation_source: "profile_preview",
                    permission: null,
                    shell_argv: [],
                    transport_override: null,
                    restore_enabled_state: null,
                  },
                }),
                current_state: "enabled",
                expected_state: "disabled",
                status: "ready",
                reason: "canonical action is ready for explicit review",
              },
              {
                action: migrated.actions[1],
                plan: planFor({
                  serial: "QA123",
                  target: args.target,
                  package: "com.example.disabled",
                  kind: "disable",
                  user_id: 0,
                  context: {
                    confirmation_source: "profile_preview",
                    permission: null,
                    shell_argv: [],
                    transport_override: null,
                    restore_enabled_state: null,
                  },
                }),
                current_state: "disabled",
                expected_state: "disabled",
                status: "already_matches",
                reason: "package is already disabled",
              },
            ],
          };
        }
        if (cmd === "install_apk") {
          // Incremental installs take a clean success path with the mode
          // recorded, independent of the downgrade-override sequence below.
          if (args.options?.incremental) {
            emitChannel(args.on_event, {
              operation_id: args.operation_id,
              kind: "progress",
              message: "Installing the APK incrementally",
            });
            return {
              succeeded: true,
              source_kind: "apk",
              install_mode: "incremental",
              file_count: 1,
              total_bytes: 12288,
              output: "Success",
              failure: null,
              audit_id: args.operation_id,
              retry_path_grant: null,
            };
          }
          installAttempts += 1;
          const expectedGrant =
            installAttempts === 1
              ? "123e4567-e89b-42d3-a456-426614174002"
              : "123e4567-e89b-42d3-a456-426614174003";
          if (args.path_grant !== expectedGrant) {
            throw new Error(
              "Install did not consume the expected one-shot grant",
            );
          }
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "progress",
            message:
              installAttempts === 1
                ? "Creating an atomic Android install session"
                : "Committing the complete package set",
          });
          if (installAttempts === 1) {
            return {
              succeeded: false,
              source_kind: "apks",
              install_mode: "normal",
              file_count: 3,
              total_bytes: 24576,
              output: "",
              failure: {
                code: "INSTALL_FAILED_VERSION_DOWNGRADE",
                cause:
                  "The selected package has a lower version code than the installed app.",
                remedy: "Use a newer build or review the downgrade override.",
                suggested_override: "allow_downgrade",
                raw_output:
                  "Failure [INSTALL_FAILED_VERSION_DOWNGRADE: Downgrade detected]",
              },
              audit_id: args.operation_id,
              retry_path_grant: "123e4567-e89b-42d3-a456-426614174003",
            };
          }
          if (
            !args.options?.override_confirmed ||
            !args.options?.allow_downgrade ||
            args.options?.bypass_low_target_sdk_block
          ) {
            throw new Error(
              "Install retry did not carry the exact confirmed downgrade override",
            );
          }
          return {
            succeeded: true,
            source_kind: "apks",
            install_mode: "normal",
            file_count: 3,
            total_bytes: 24576,
            output: "Success",
            failure: null,
            audit_id: args.operation_id,
            retry_path_grant: null,
          };
        }
        if (cmd === "list_users") {
          return [
            { id: 0, name: "Owner", running: true, current: true },
            { id: 10, name: "Work profile", running: true, current: false },
          ];
        }
        if (cmd === "journal_list") {
          return [
            {
              id: 1,
              applied: {
                plan: {
                  request: {
                    serial: "QA123",
                    package: "com.example.disabled",
                    kind: "disable",
                  },
                  args: [
                    "pm",
                    "disable-user",
                    "--user",
                    "0",
                    "com.example.disabled",
                  ],
                  description: "Disable com.example.disabled",
                },
                stdout: "Package com.example.disabled new state: disabled-user",
                applied_at: "2026-06-29T10:00:00Z",
              },
              undone_by: null,
              undoes: null,
              outcome: "succeeded",
              failure: null,
            },
            ...runtimeJournal,
          ];
        }
        if (cmd === "journal_undo") {
          const original = runtimeJournal.find(
            (entry) => entry.id === args.entry_id,
          );
          if (!original || original.applied.plan.request.kind !== "archive") {
            throw new Error("journal entry is not safely undoable");
          }
          const pkg = packages.find(
            (item) => item.package === original.applied.plan.request.package,
          );
          if (pkg) {
            pkg.archived = false;
            pkg.enabled = original.applied.before_state.endsWith("_enabled");
          }
          const undo = {
            id: ++journalId,
            applied: {
              plan: planFor({
                ...original.applied.plan.request,
                kind: "request_unarchive",
              }),
              stdout: "Success",
              before_state: "archived",
              after_state: original.applied.before_state,
              applied_at: "2026-07-15T12:30:00Z",
            },
            undone_by: null,
            undoes: original.id,
            outcome: "succeeded",
            failure: null,
          };
          original.undone_by = undo.id;
          runtimeJournal.push(undo);
          return undo;
        }
        if (cmd === "plan_action") {
          return planFor(args.request);
        }
        if (cmd === "plan_shell_action") {
          window.__DROIDSMITH_LAST_PRIVILEGED_TARGET__ = args.request.target;
          if (
            ["legacy_tcp", "unknown_tcp"].includes(
              args.request.target.transport_kind,
            ) &&
            !args.request.target.untrusted_transport_override
          ) {
            throw new Error("untrusted_transport_override_required");
          }
          const shellArgv = args.request.argv;
          const readOnly = shellArgv[0] === "getprop";
          return {
            mutating: !readOnly,
            dangerous: !readOnly,
            plan: readOnly
              ? null
              : {
                  request: {
                    serial: "QA123",
                    target: args.request.target,
                    package: "",
                    kind: "shell",
                    user_id: 0,
                    pack_context: null,
                    context: {
                      confirmation_source: "console_review",
                      permission: null,
                      shell_argv: shellArgv,
                      transport_override: null,
                    },
                  },
                  args: shellArgv,
                  description: `Run reviewed shell mutation: ${shellArgv.join(" ")}`,
                  incident_id: `op-ui-console-${journalId + 1}`,
                  before_state: "not_captured",
                },
          };
        }
        if (cmd === "shell_run") {
          if (args.argv[0] === "getprop") {
            emitChannel(args.on_event, {
              operation_id: args.operation_id,
              kind: "started",
              message: "Running ADB shell command",
            });
            emitChannel(args.on_event, {
              operation_id: args.operation_id,
              kind: "output",
              stream: "stdout",
              chunk: "Pixel QA\n",
            });
            return "Pixel QA\n";
          }
          throw new Error("Mock shell_run only allows read-only commands");
        }
        if (cmd === "stream_logcat") {
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "started",
            message: "Logcat stream started",
          });
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "output",
            stream: "stdout",
            chunk: "I/QA(  123): first part",
          });
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "output",
            stream: "stdout",
            chunk: " complete\n",
          });
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "reconnecting",
            message: "Logcat disconnected; reconnecting",
            attempt: 2,
          });
          emitChannel(args.on_event, {
            operation_id: args.operation_id,
            kind: "output",
            stream: "stdout",
            chunk: "W/QA(  123): after reconnect\n",
          });
          return new Promise((resolve) => {
            pendingOperations.set(args.operation_id, {
              resolve,
              channel: args.on_event,
            });
          });
        }
        if (cmd === "cancel_operation") {
          const pending = pendingOperations.get(args.operation_id);
          if (!pending) return false;
          pendingOperations.delete(args.operation_id);
          emitChannel(pending.channel, {
            operation_id: args.operation_id,
            kind: "cancelled",
            message: "Operation cancelled",
          });
          pending.resolve();
          return true;
        }
        if (cmd === "explain_failure") {
          if (
            (args.req?.raw_error ?? "").includes("OEM policy blocked") ||
            (args.req?.package_id ?? "").includes("fail")
          ) {
            return {
              id: "qa-oem-block",
              title: "Vendor blocks disabling this package",
              explanation:
                "This OEM ROM prevents the shell user from disabling the package. Uninstall for the current user instead.",
              matches: null,
              mitigation: { kind: "try_alternative_action" },
            };
          }
          return null;
        }
        if (cmd === "apply_action") {
          const request = args.plan.request;
          if (request.package === "com.example.fail") {
            throw new Error("OEM policy blocked this package");
          }
          await new Promise((resolve) => window.setTimeout(resolve, 150));
          const pkg = packages.find((item) => item.package === request.package);
          const beforeState =
            request.kind === "archive"
              ? pkg?.enabled
                ? "user_installed_enabled"
                : "user_installed_disabled"
              : request.kind === "request_unarchive"
                ? "archived"
                : "installed_enabled";
          if (pkg && request.kind === "disable") pkg.enabled = false;
          if (pkg && request.kind === "enable") pkg.enabled = true;
          if (pkg && request.kind === "archive") {
            pkg.archived = true;
            pkg.enabled = false;
          }
          if (pkg && request.kind === "request_unarchive") {
            pkg.archived = false;
            pkg.enabled = true;
          }
          const afterState =
            request.kind === "archive"
              ? "archived"
              : request.kind === "request_unarchive"
                ? "user_installed_enabled"
                : "installed_disabled";
          const stdout = `Applied ${request.kind} to ${request.package}`;
          const entry = {
            id: ++journalId,
            applied: {
              plan: args.plan,
              stdout,
              before_state: beforeState,
              after_state: afterState,
              applied_at: "2026-06-29T10:05:00Z",
            },
            undone_by: null,
            undoes: null,
            outcome: "succeeded",
            failure: null,
          };
          runtimeJournal.push(entry);
          return { entry, stdout };
        }
        if (cmd === "list_packs") {
          return {
            errors: [],
            packs: [
              {
                pack: {
                  id: "qa-debloat",
                  revision: 3,
                  name: "QA Debloat Pack",
                  version: "1",
                  description:
                    "Synthetic pack used by rendered route smoke tests.",
                  targets: {
                    manufacturer: ["Google"],
                    rom: ["Pixel"],
                    model: [],
                    build_fingerprint: ["google/"],
                    android_min: 13,
                    android_max: null,
                    user_scope: "owner",
                  },
                  packages: [
                    {
                      id: "com.example.app",
                      removal: "recommended",
                      description: "Safe QA package.",
                      depends_on: [],
                      needed_by: [],
                      labels: ["qa", "telemetry"],
                    },
                    {
                      id: "com.example.fail",
                      removal: "recommended",
                      description:
                        "Package that simulates an OEM policy failure.",
                      depends_on: [],
                      needed_by: [],
                      labels: ["qa", "failure"],
                    },
                    {
                      id: "com.android.settings",
                      removal: "unsafe",
                      description:
                        "System settings is intentionally not preselected.",
                      depends_on: [],
                      needed_by: ["device settings"],
                      labels: ["system"],
                    },
                  ],
                  attribution: null,
                  provenance: { source: "ui-smoke", license: "MIT" },
                },
                assessment: qaPackAssessment,
                imported: false,
              },
              {
                pack: {
                  id: "qa-imported",
                  revision: 1,
                  name: "QA Imported Pack",
                  version: "1",
                  description:
                    "Synthetic locally-imported pack used by rendered route smoke tests.",
                  targets: {
                    manufacturer: ["Google"],
                    rom: ["Pixel"],
                    model: [],
                    build_fingerprint: ["google/"],
                    android_min: 13,
                    android_max: null,
                    user_scope: "owner",
                  },
                  packages: [
                    {
                      id: "com.example.imported",
                      removal: "recommended",
                      description: "Imported QA package.",
                      depends_on: [],
                      needed_by: [],
                      labels: ["qa"],
                    },
                  ],
                  attribution: null,
                  provenance: { source: "ui-smoke", license: "MIT" },
                },
                assessment: qaPackAssessment,
                imported: true,
              },
            ],
          };
        }
        if (cmd === "remove_imported_pack") {
          return true;
        }
        if (cmd === "export_device_pack") {
          return {
            pack_id: "qa-device-export",
            packages: 2,
            artifact: {
              local_path: "C:/Users/QA/Desktop/device-debloat.yaml",
              size_bytes: 512,
              sha256:
                "1111111111111111111111111111111111111111111111111111111111111111",
            },
          };
        }
        if (cmd === "analyze_apk") {
          return {
            file_name: "sample.apk",
            file_size: 4_215_872,
            sha256:
              "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            package: "com.example.qa",
            version_code: 4207,
            version_name: "4.2.0",
            min_sdk: 24,
            target_sdk: 34,
            compile_sdk: 34,
            permissions: [
              "android.permission.INTERNET",
              "android.permission.POST_NOTIFICATIONS",
            ],
            components: {
              activities: 12,
              services: 3,
              receivers: 5,
              providers: 2,
            },
            dex: {
              files: 2,
              defined_classes: 8123,
              method_refs: 71204,
              exceeds_64k: true,
            },
            signing: { v1: false, v2: true, v3: true, v31: false },
            total_entries: 512,
            largest_entries: [
              {
                name: "classes.dex",
                compressed: 1_800_000,
                uncompressed: 5_400_000,
              },
              {
                name: "resources.arsc",
                compressed: 900_000,
                uncompressed: 1_200_000,
              },
            ],
          };
        }
        if (cmd === "plan_pack") {
          const selected = args.request.selected;
          const packContext = {
            pack_id: "qa-debloat",
            revision: 3,
            provenance_source: "ui-smoke",
            provenance_license: "MIT",
            compatibility_status: "compatible",
            override_accepted: false,
          };
          return {
            pack_id: "qa-debloat",
            revision: 3,
            assessment: qaPackAssessment,
            selected_ids: selected,
            plans: selected.map((packageId) =>
              planFor({
                serial: "QA123",
                target: args.request.target,
                package: packageId,
                kind: "disable",
                user_id: args.request.user_id,
                pack_context: packContext,
              }),
            ),
            skipped: [],
          };
        }
        if (cmd === "locate_scrcpy") return "C:/Tools/scrcpy.exe";
        if (cmd === "locate_gnirehtet") return "C:/Tools/gnirehtet.exe";
        if (cmd === "find_gnirehtet_session") {
          // Model a session already running for this device so the panel
          // re-attaches on mount (persist tethering across navigation).
          if (gnirehtetStopped) return null;
          return {
            id: 501,
            serial: args.target.serial,
            pid: 6001,
            args: ["run", args.target.serial],
            started_at: "2026-07-21T09:00:00Z",
            state: "running",
            exit_code: null,
            exit_reason: null,
            stderr_tail: "",
          };
        }
        if (cmd === "gnirehtet_session_status") {
          return {
            id: args.session_id,
            serial: "R58N-QA",
            pid: 6001,
            args: ["run", "R58N-QA"],
            started_at: "2026-07-21T09:00:00Z",
            state: gnirehtetStopped ? "stopped" : "running",
            exit_code: gnirehtetStopped ? 0 : null,
            exit_reason: gnirehtetStopped ? "user_stopped" : null,
            stderr_tail: "",
          };
        }
        if (cmd === "stop_gnirehtet") {
          gnirehtetStopped = true;
          return {
            id: args.session_id,
            serial: "R58N-QA",
            pid: 6001,
            args: ["run", "R58N-QA"],
            started_at: "2026-07-21T09:00:00Z",
            state: "stopped",
            exit_code: 0,
            exit_reason: "user_stopped",
            stderr_tail: "",
          };
        }
        if (cmd === "scrcpy_capabilities") {
          return {
            path: "C:/Tools/scrcpy.exe",
            version: "4.0",
            available_video_codecs: ["h264", "h265"],
            video_encoders: [
              {
                codec: "h264",
                name: "c2.vendor.avc.encoder",
                software: false,
              },
              {
                codec: "h265",
                name: "c2.vendor.hevc.encoder",
                software: false,
              },
            ],
            probe_warning: null,
            cache_hit: false,
            supports_flex_display: true,
            supports_keep_active: true,
            supports_new_display: true,
            supports_audio_source_expansion: true,
            supports_camera: true,
          };
        }
        if (cmd === "launch_scrcpy") {
          if (
            args.path_grant !== "123e4567-e89b-42d3-a456-426614174009" ||
            "record_path" in args.request ||
            args.request.video_codec !== "h265" ||
            args.request.video_encoder !== "c2.vendor.hevc.encoder"
          ) {
            throw new Error(
              "Mirror launch bypassed the native grant or negotiated capabilities",
            );
          }
          scrcpyLaunches += 1;
          return {
            id: 301,
            serial: args.request.serial,
            pid: 4242,
            args: [
              "-s",
              args.request.serial,
              "--video-codec=h265",
              "--video-encoder=c2.vendor.hevc.encoder",
              "--record",
              "C:/Users/QA/Desktop/droidsmith-recording-2026-07-15.mp4",
            ],
            started_at: "2026-07-15T11:00:00Z",
            state: "running",
            exit_code: null,
            exit_reason: null,
            stderr_tail: "",
          };
        }
        if (cmd === "scrcpy_session_status") {
          if (scrcpyLaunches >= 2) {
            return {
              id: args.session_id,
              serial: "QA123",
              pid: 4243,
              args: [
                "-s",
                "QA123",
                "--video-codec=h265",
                "--video-encoder=c2.vendor.hevc.encoder",
              ],
              started_at: "2026-07-15T11:01:00Z",
              state: "exited",
              exit_code: 1,
              exit_reason: "encoder_failed",
              stderr_tail:
                "[server] ERROR: MediaCodec encoder failed to initialize",
            };
          }
          return {
            id: args.session_id,
            serial: "QA123",
            pid: 4242,
            args: ["-s", "QA123"],
            started_at: "2026-07-15T11:00:00Z",
            state: "running",
            exit_code: null,
            exit_reason: null,
            stderr_tail: "",
          };
        }
        if (cmd === "stop_scrcpy") {
          return {
            id: args.session_id,
            serial: "QA123",
            pid: 4242,
            args: ["-s", "QA123"],
            started_at: "2026-07-15T11:00:00Z",
            state: "stopped",
            exit_code: 0,
            exit_reason: "user_stopped",
            stderr_tail: "INFO: session stopped by user",
          };
        }
        if (cmd === "locate_fastboot") return null;
        if (cmd === "list_fastboot_devices") return [];
        if (cmd === "list_permissions") return [];
        if (cmd === "initialize_settings") {
          return {
            settings: {
              version: "1",
              language: settingsLanguage,
              mirrorPresetCount: 0,
            },
            recovery: "clean",
            legacyCleanupAllowed: false,
          };
        }
        if (cmd === "set_settings_language") {
          settingsLanguage = args.language;
          return {
            version: "1",
            language: settingsLanguage,
            mirrorPresetCount: 0,
          };
        }
        if (cmd === "get_settings_mirror_preset") return null;
        if (
          cmd === "set_settings_mirror_preset" ||
          cmd === "reset_settings_mirror_preset" ||
          cmd === "reset_settings"
        ) {
          if (cmd === "reset_settings" && args.scope !== "mirror_presets") {
            settingsLanguage = null;
          }
          return {
            version: "1",
            language: settingsLanguage,
            mirrorPresetCount: 0,
          };
        }
        if (cmd === "export_settings") {
          return {
            path: "C:/Users/QA/Desktop/droidsmith-settings-all.json",
            byteSize: 128,
            scope: args.scope,
          };
        }
        if (cmd === "has_settings_import_backup") {
          return settingsImportBackupAvailable;
        }
        if (cmd === "preview_settings_import") {
          const counts = {
            added: 1,
            updated: 0,
            removed: 0,
            unchanged: 0,
          };
          return {
            importId: "123e4567-e89b-42d3-a456-426614174012",
            version: "1",
            scope: "all",
            merge: {
              languageChanged: false,
              mirrorPresets: counts,
              logcatQueries: counts,
              wirelessEndpoints: counts,
              autoReconnectChanged: true,
            },
            replace: {
              languageChanged: false,
              mirrorPresets: counts,
              logcatQueries: counts,
              wirelessEndpoints: counts,
              autoReconnectChanged: true,
            },
            excludedMachineLocal: ["deviceFingerprints"],
            backupAvailable: settingsImportBackupAvailable,
          };
        }
        if (cmd === "apply_settings_import") {
          settingsImportBackupAvailable = true;
          return {
            settings: {
              version: "1",
              language: settingsLanguage,
              mirrorPresetCount: 1,
            },
            mode: args.mode,
            backupAvailable: true,
          };
        }
        if (cmd === "restore_settings_import_backup") {
          return {
            version: "1",
            language: settingsLanguage,
            mirrorPresetCount: 0,
          };
        }
        if (cmd === "capture_layout") {
          return {
            node_count: 2,
            raw_xml:
              '<hierarchy rotation="0"><node index="0" class="android.widget.FrameLayout" package="com.example.app" text="" content-desc="" resource-id="" bounds="[0,0][1080,2400]" clickable="false" enabled="true"><node index="1" class="android.widget.TextView" package="com.example.app" text="Hello" content-desc="Greeting" resource-id="com.example.app:id/title" bounds="[10,20][300,80]" clickable="true" enabled="true" /></node></hierarchy>',
            nodes: [
              {
                depth: 0,
                index: "0",
                class: "android.widget.FrameLayout",
                package: "com.example.app",
                text: "",
                content_desc: "",
                resource_id: "",
                bounds: "[0,0][1080,2400]",
                clickable: false,
                enabled: true,
              },
              {
                depth: 1,
                index: "1",
                class: "android.widget.TextView",
                package: "com.example.app",
                text: "Hello",
                content_desc: "Greeting",
                resource_id: "com.example.app:id/title",
                bounds: "[10,20][300,80]",
                clickable: true,
                enabled: true,
              },
            ],
          };
        }
        if (cmd === "save_layout_export") {
          return "C:/Users/QA/Desktop/layout-QA123.xml";
        }
        if (cmd === "list_logcat_queries") {
          return { version: "1", global: logcatQueries.global, device: [] };
        }
        if (cmd === "save_logcat_queries") {
          if (args.scope === "global") logcatQueries.global = args.queries;
          return {
            version: "1",
            global: logcatQueries.global,
            device: args.scope === "device" ? args.queries : [],
          };
        }
        throw new Error(`Unhandled mocked command: ${cmd}`);
      },
      transformCallback(callback) {
        const id = nextCallbackId++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback(id) {
        callbacks.delete(id);
      },
      runCallback(id, data) {
        callbacks.get(id)?.(data);
      },
      convertFileSrc(filePath) {
        return filePath;
      },
      // getCurrentWindow()/getCurrentWebview() read these labels; without the
      // metadata block they throw "reading 'currentWindow'".
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { windowLabel: "main", label: "main" },
      },
    };

    // The event plugin's unlisten path lives on its own global; without it the
    // Apps drag-drop cleanup throws "reading 'unregisterListener'". This is a
    // no-op: `plugin:event|listen` returns a dedicated event id (nextEventId)
    // that is NOT a `callbacks` key, so deleting by it here would evict an
    // unrelated transformCallback entry (e.g. the watch_devices channel) and
    // silently break device-snapshot delivery.
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener() {},
    };

    function emitChannel(channel, message) {
      const id = channel?.id;
      const callback = callbacks.get(id);
      if (!callback) return;
      const index = channelIndexes.get(id) ?? 0;
      channelIndexes.set(id, index + 1);
      callback({ index, message });
    }

    function filterPackages(items, filter) {
      if (filter === "user") return items.filter((item) => !item.system);
      if (filter === "system") return items.filter((item) => item.system);
      if (filter === "enabled")
        return items.filter((item) => item.enabled && !item.archived);
      if (filter === "disabled")
        return items.filter((item) => !item.enabled && !item.archived);
      if (filter === "archived") return items.filter((item) => item.archived);
      return items;
    }

    function packageExportResult(
      mode,
      localPath,
      legacyContent,
      artifactCount,
    ) {
      return {
        artifact: {
          local_path: localPath,
          size_bytes: 8192,
          sha256: "d".repeat(64),
        },
        manifest: {
          format: "droidsmith_package_export",
          schema_version: 1,
          created_at: "2026-07-15T09:00:00Z",
          mode,
          package: "com.example.app",
          android_user: 0,
          device: {
            device_identity_sha256: "e".repeat(64),
            build_identity_sha256: "f".repeat(64),
          },
          eligibility: {
            device_sdk: 35,
            target_sdk: 30,
            debuggable: false,
            allow_backup: true,
            reason:
              "Package targets an API below the Android 12 exclusion threshold.",
          },
          legacy_content: legacyContent,
          artifacts: Array.from({ length: artifactCount }, (_, index) => ({
            name: index === 0 ? "base.apk" : `split-${index}.apk`,
            role: mode === "apk_export" ? "apk" : "legacy_android_backup",
            size_bytes: 4096,
            sha256: String(index + 1).repeat(64),
          })),
        },
      };
    }

    function planFor(request) {
      const action =
        request.kind === "enable"
          ? ["pm", "enable", request.package]
          : request.kind === "archive"
            ? [
                "pm",
                "archive",
                "--user",
                String(request.user_id),
                request.package,
              ]
            : request.kind === "request_unarchive"
              ? [
                  "pm",
                  "request-unarchive",
                  "--user",
                  String(request.user_id),
                  request.package,
                ]
              : ["pm", "disable-user", "--user", "0", request.package];
      return {
        request: {
          ...request,
          context: request.context ?? {
            confirmation_source: "apps_preview",
            permission: null,
            shell_argv: [],
            transport_override: null,
          },
        },
        args: action,
        description: `${request.kind} ${request.package}`,
        incident_id: `op-ui-smoke-${journalId + 1}`,
        before_state: "installed_enabled",
      };
    }

    function remoteFilePlan(request) {
      const destination = request.destination_path ?? null;
      const argv =
        request.kind === "mkdir"
          ? ["mkdir", request.source_path]
          : request.kind === "rename"
            ? ["mv", "-n", request.source_path, destination]
            : request.kind === "delete_directory"
              ? ["rm", "-rf", request.source_path]
              : ["rm", "-f", request.source_path];
      return {
        kind: request.kind,
        source_path: request.source_path,
        destination_path: destination,
        argv,
        description: `Reviewed ${request.kind} operation`,
        destructive: request.kind !== "mkdir",
      };
    }

    function fileMutationResult(plan) {
      const actionPlan = {
        request: {
          serial: device.serial,
          target: {
            serial: device.serial,
            transport_id: device.transport_id,
            connection_generation: device.connection_generation,
            transport_kind: device.transport_kind,
            untrusted_transport_override: false,
            model: device.model,
            product: device.product,
            device: device.device,
            build_fingerprint: device.build_fingerprint,
          },
          package: "",
          kind: "shell",
          user_id: 0,
          pack_context: null,
          context: {
            confirmation_source: "file_manager_review",
            permission: null,
            shell_argv: plan.argv,
            transport_override: null,
            restore_enabled_state: null,
          },
        },
        args: plan.argv,
        description: plan.description,
        incident_id: `file-ui-smoke-${journalId + 1}`,
        before_state: "present",
      };
      const stdout = "File operation completed";
      return {
        stdout,
        entry: {
          id: ++journalId,
          applied: {
            plan: actionPlan,
            stdout,
            display_stdout: stdout,
            before_state: "present",
            after_state: "present",
            applied_at: "2026-07-15T12:00:00Z",
          },
          undone_by: null,
          undoes: null,
          outcome: "succeeded",
          failure: null,
        },
      };
    }
  });
}

function startVite() {
  const command = npmCommand("dev", [
    "--",
    "--host",
    "127.0.0.1",
    "--port",
    uiSmokePort,
    "--strictPort",
  ]);
  const child = spawn(command.file, command.args, {
    cwd: repoRoot,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.on("data", (chunk) => stdout.write(chunk));
  child.stderr.on("data", (chunk) => stdout.write(chunk));
  return child;
}

function npmCommand(scriptName, extraArgs = []) {
  if (env.npm_execpath && fs.existsSync(env.npm_execpath)) {
    return {
      file: execPath,
      args: [env.npm_execpath, "run", scriptName, ...extraArgs],
    };
  }
  if (platform === "win32") {
    const command = ["npm", "run", scriptName, ...extraArgs]
      .map((part) => (part.includes(" ") ? `"${part}"` : part))
      .join(" ");
    return { file: "cmd.exe", args: ["/d", "/s", "/c", command] };
  }
  return { file: "npm", args: ["run", scriptName, ...extraArgs] };
}

async function waitForHttp(url) {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    if (await canReach(url)) return;
    await delay(250);
  }
  throw new Error(`Timed out waiting for ${url}`);
}

function canReach(url) {
  return new Promise((resolve) => {
    const req = http.get(url, (res) => {
      res.resume();
      resolve(Boolean(res.statusCode && res.statusCode < 500));
    });
    req.on("error", () => resolve(false));
    req.setTimeout(1000, () => {
      req.destroy();
      resolve(false);
    });
  });
}

function stopServer(child) {
  if (child.exitCode !== null) return;
  if (platform === "win32" && child.pid) {
    spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], {
      stdio: "ignore",
    });
    return;
  }
  child.kill();
}

function findOpenPort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : 0;
      server.close(() => resolve(port));
    });
  });
}
