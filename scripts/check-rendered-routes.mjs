/* global window, document, getComputedStyle, HTMLElement */
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { env, execPath, platform, stdout } from "node:process";
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

fs.mkdirSync(screenshotDir, { recursive: true });

const server = startVite();

try {
  await waitForHttp(baseUrl);

  const browser = await chromium.launch();
  try {
    await runDesktopFlow(browser);
    await runMobileFlow(browser);
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
  await page.getByRole("button", { name: "Review recovery" }).click();
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
  await page.getByRole("button", { name: "Diagnostics", exact: true }).click();
  await page.getByRole("dialog", { name: "Diagnostics center" }).waitFor();
  await page.getByText("Nothing is uploaded", { exact: true }).waitFor();
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
  for (const route of ["Devices", "Apps", "Debloat", "Console"]) {
    await page.getByRole("button", { name: new RegExp(route) }).click();
    await page.getByRole("heading", { name: route, exact: true }).waitFor();
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
  await page.getByRole("button", { name: "Start tail" }).click();
  await page.getByText(/I\/QA\(\s*123\): first part complete/).waitFor();
  await page
    .getByText(/Logcat disconnected; reconnecting \(attempt 2\)/)
    .waitFor();
  await page.getByText(/W\/QA\(\s*123\): after reconnect/).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Logcat stream");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-logcat-stream.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.getByRole("button", { name: "Start tail" }).waitFor();

  await page.getByRole("button", { name: /Apps/ }).click();
  await page.getByText("com.example.app").waitFor();
  await page.getByText("com.android.settings").waitFor();
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(false));
  await page.getByText("No authorized devices", { exact: true }).waitFor();
  await page.getByText("com.example.app").waitFor({ state: "hidden" });
  await page.evaluate(() => window.__DROIDSMITH_MOCK_HOTPLUG__(true));
  await page.getByText("com.example.app").waitFor();
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

  await page
    .getByRole("row")
    .filter({ hasText: "com.example.app" })
    .getByRole("button", { name: "Disable" })
    .click();
  await page.getByRole("alertdialog").waitFor();
  await page.getByText(/pm disable-user --user 0 com\.example\.app/).waitFor();
  await assertTabMovesFocus(page, "Apps action overlay");
  await page.getByRole("button", { name: "Cancel" }).click();

  await page.getByRole("button", { name: "Commands", exact: true }).click();
  await page.getByRole("dialog", { name: "Command palette" }).waitFor();
  await assertFocusedLabel(page, "Command palette search");
  await page.getByLabel("Command palette search").fill("debloat");
  await page.getByRole("option", { name: /Debloat/ }).click();
  await page.getByRole("heading", { name: "Debloat", exact: true }).waitFor();

  await page.getByRole("button", { name: /QA Debloat Pack/ }).click();
  await page.getByRole("heading", { name: "Compatibility checks" }).waitFor();
  await page.getByText(/Pack qa-debloat · revision 3 · MIT/).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Debloat pack preview");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-debloat-preview.png"),
    fullPage: false,
  });
  await page.getByRole("button", { name: /Apply 2 packages/ }).click();
  await page.getByText(/QA Debloat Pack - debloat complete/).waitFor();
  await page.getByText("Failed", { exact: true }).waitFor();
  await page.getByRole("button", { name: /Retry 1 failed/ }).waitFor();
  await assertNoHorizontalOverflow(page, "desktop Debloat queue");
  await page.screenshot({
    path: path.join(screenshotDir, "desktop-debloat-queue.png"),
    fullPage: false,
  });

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

async function assertNoHorizontalOverflow(page, label) {
  const offenders = await page
    .locator(
      "button, select, input, label, h1, h2, h3, h4, p, [role='option'], [role='radio']",
    )
    .evaluateAll((elements) =>
      elements.flatMap((element) => {
        if (!(element instanceof HTMLElement)) return [];
        if (element.closest(".overflow-x-auto")) return [];
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

async function installTauriMock(page) {
  await page.addInitScript(() => {
    const callbacks = new Map();
    const channelIndexes = new Map();
    const pendingOperations = new Map();
    let nextCallbackId = 1;
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
    };
    const packages = [
      {
        package: "com.example.app",
        enabled: true,
        system: false,
        apk_path: "/data/app/com.example.app/base.apk",
        uid: 10101,
        installer: "com.android.vending",
      },
      {
        package: "com.android.settings",
        enabled: true,
        system: true,
        apk_path: "/system/priv-app/Settings/Settings.apk",
        uid: 1000,
        installer: null,
      },
      {
        package: "com.example.disabled",
        enabled: false,
        system: false,
        apk_path: "/data/app/com.example.disabled/base.apk",
        uid: 10102,
        installer: null,
      },
      {
        package: "com.example.fail",
        enabled: true,
        system: false,
        apk_path: "/data/app/com.example.fail/base.apk",
        uid: 10103,
        installer: null,
      },
    ];
    let journalId = 20;
    let installAttempts = 0;
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
        if (cmd === "select_host_path") {
          const selections = {
            diagnostics_save: {
              id: "123e4567-e89b-42d3-a456-426614174001",
              local_path: "C:/Users/QA/Desktop/droidsmith-support.json",
            },
            install_open: {
              id: "123e4567-e89b-42d3-a456-426614174002",
              local_path: "C:/Users/QA/Downloads/sample.apks",
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
            },
          };
        }
        if (cmd === "list_devices") {
          return {
            adb_resolved: true,
            adb_path: "C:/Android/platform-tools/adb.exe",
            devices: [device],
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
        if (cmd === "wipe_diagnostics") {
          return {
            files_removed: 2,
            bytes_removed: 4096,
            device_journals_preserved: true,
          };
        }
        if (cmd === "list_packages") {
          return filterPackages(packages, args.filter ?? "all");
        }
        if (cmd === "install_apk") {
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
            },
          ];
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
        if (cmd === "apply_action") {
          const request = args.plan.request;
          if (request.package === "com.example.fail") {
            throw new Error("OEM policy blocked this package");
          }
          const pkg = packages.find((item) => item.package === request.package);
          if (pkg && request.kind === "disable") pkg.enabled = false;
          if (pkg && request.kind === "enable") pkg.enabled = true;
          const stdout = `Applied ${request.kind} to ${request.package}`;
          const entry = {
            id: ++journalId,
            applied: {
              plan: args.plan,
              stdout,
              before_state: "installed_enabled",
              after_state: "installed_disabled",
              applied_at: "2026-06-29T10:05:00Z",
            },
            undone_by: null,
            undoes: null,
          };
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
                      labels: ["qa"],
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
                      removal: "expert",
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
        if (cmd === "locate_scrcpy" || cmd === "locate_fastboot") return null;
        if (cmd === "list_fastboot_devices") return [];
        if (cmd === "list_permissions") return [];
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
      if (filter === "enabled") return items.filter((item) => item.enabled);
      if (filter === "disabled") return items.filter((item) => !item.enabled);
      return items;
    }

    function planFor(request) {
      const action =
        request.kind === "enable"
          ? ["pm", "enable", request.package]
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
