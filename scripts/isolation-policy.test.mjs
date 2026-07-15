import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { URL } from "node:url";
import vm from "node:vm";

const source = await readFile(
  new URL("../isolation/index.js", import.meta.url),
  "utf8",
);
const tauriConfig = JSON.parse(
  await readFile(
    new URL("../src-tauri/tauri.conf.json", import.meta.url),
    "utf8",
  ),
);
const tauriLib = await readFile(
  new URL("../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);
const sandbox = { window: {} };
vm.runInNewContext(source, sandbox, { filename: "isolation/index.js" });
const hook = sandbox.window.__TAURI_ISOLATION_HOOK__;
const blockedCommand = "__droidsmith_isolation_rejected__";
const pathGrant = "123e4567-e89b-42d3-a456-426614174000";

const target = Object.freeze({
  serial: "R58M12345",
  transport_id: 7,
  connection_generation: 2,
  transport_kind: "usb",
  untrusted_transport_override: false,
  model: "Pixel 9",
  product: "tokay",
  device: "tokay",
  build_fingerprint: "google/tokay/tokay:17/test:user/release-keys",
});

function message(cmd, payload) {
  return { cmd, callback: 1, error: 2, options: {}, payload };
}

test("production config enables isolation with a strict CSP", () => {
  const security = tauriConfig.app.security;
  assert.equal(security.pattern.use, "isolation");
  assert.equal(security.pattern.options.dir, "../isolation");
  assert.equal(security.csp.includes("'unsafe-inline'"), false);
  assert.equal(security.csp.includes("'unsafe-eval'"), false);
  assert.match(security.csp, /script-src 'self'/u);
  assert.match(security.csp, /style-src 'self'/u);
  assert.match(security.csp, /object-src 'none'/u);
});

test("every registered Rust command has an explicit isolation classification", () => {
  const handlerBlock = tauriLib.match(
    /invoke_handler\(tauri::generate_handler!\[([\s\S]*?)\]\)/u,
  )?.[1];
  const readOnlyBlock = source.match(
    /READ_ONLY_COMMANDS = new Set\(\[([\s\S]*?)\]\);/u,
  )?.[1];
  const sensitiveBlock = source.match(
    /SENSITIVE_COMMANDS = Object\.freeze\(\{([\s\S]*?)^ {2}\}\);/mu,
  )?.[1];
  assert.ok(handlerBlock);
  assert.ok(readOnlyBlock);
  assert.ok(sensitiveBlock);

  const registered = new Set(
    handlerBlock
      .match(/[a-z][a-z0-9_]*/gu)
      ?.filter((name) => name !== "tauri") ?? [],
  );
  const readOnly = new Set(
    [...readOnlyBlock.matchAll(/"([a-z][a-z0-9_]*)"/gu)].map(
      (match) => match[1],
    ),
  );
  const sensitive = new Set(
    [...sensitiveBlock.matchAll(/^ {4}([a-z][a-z0-9_]*):/gmu)].map(
      (match) => match[1],
    ),
  );
  const classified = new Set([...readOnly, ...sensitive]);
  assert.deepEqual([...classified].sort(), [...registered].sort());
});

test("passes known read-only and plugin commands unchanged", () => {
  const readOnly = message("list_devices", {});
  const plugin = message("plugin:dialog|save", { options: {} });
  assert.equal(hook(readOnly), readOnly);
  assert.equal(hook(plugin), plugin);
});

test("passes a schema-valid sensitive command", () => {
  const valid = message("pull_file", {
    target,
    remote_path: "/sdcard/Download/report.txt",
    path_grant: pathGrant,
    operation_id: "pull-123",
    on_event: 99,
  });
  assert.equal(hook(valid), valid);
});

test("rejects malformed path grants without echoing them", () => {
  for (const invalidGrant of ["not-a-grant", "/tmp/../etc/passwd"]) {
    const result = hook(
      message("take_screenshot", { target, path_grant: invalidGrant }),
    );
    assert.equal(result.cmd, blockedCommand);
    assert.equal(result.payload.code, "ipc_policy_rejected");
    assert.equal(JSON.stringify(result).includes(invalidGrant), false);
  }
});

test("rejects remote traversal and unexpected command fields", () => {
  const traversal = hook(
    message("pull_file", {
      target,
      remote_path: "/sdcard/../data/secret",
      path_grant: pathGrant,
      operation_id: "pull-456",
      on_event: 99,
    }),
  );
  const unexpected = hook(
    message("take_screenshot", {
      target,
      path_grant: pathGrant,
      arbitrary_path: "/etc/passwd",
    }),
  );
  assert.equal(traversal.cmd, blockedCommand);
  assert.equal(unexpected.cmd, blockedCommand);
});

test("rejects malformed targets and future mutation commands by default", () => {
  const malformed = hook(
    message("take_screenshot", {
      target: { ...target, connection_generation: 0 },
      path_grant: pathGrant,
    }),
  );
  const unknown = hook(message("delete_remote_file", { path: "/sdcard/x" }));
  assert.equal(malformed.cmd, blockedCommand);
  assert.equal(unknown.cmd, blockedCommand);
});

test("validates transport provenance and explicit override fields", () => {
  for (const malformedTarget of [
    { ...target, transport_kind: "wifi" },
    { ...target, untrusted_transport_override: "yes" },
  ]) {
    assert.equal(
      hook(
        message("take_screenshot", {
          target: malformedTarget,
          path_grant: pathGrant,
        }),
      ).cmd,
      blockedCommand,
    );
  }
});

test("rejects option-like wireless hosts and non-string pairing codes", () => {
  const host = hook(
    message("connect_wireless", {
      request: { host: "-a", port: 5555, legacy_tcp: false },
    }),
  );
  const code = hook(
    message("pair_wireless", {
      request: { host: "pixel.local", port: 37001, pairing_code: 123456 },
    }),
  );
  assert.equal(host.cmd, blockedCommand);
  assert.equal(code.cmd, blockedCommand);
});

test("requires an explicit legacy TCP classification on wireless connect", () => {
  const valid = message("connect_wireless", {
    request: { host: "pixel.local", port: 5555, legacy_tcp: true },
  });
  const missing = message("connect_wireless", {
    request: { host: "pixel.local", port: 5555 },
  });
  assert.equal(hook(valid), valid);
  assert.equal(hook(missing).cmd, blockedCommand);
});

test("accepts only a one-shot grant for optional scrcpy recording", () => {
  const request = {
    serial: target.serial,
    target,
    max_size: 1280,
    bit_rate: "8M",
    no_audio: false,
    keyboard_mode: "uhid",
    video_codec: "h265",
    video_encoder: "c2.vendor.hevc.encoder",
    turn_screen_off: false,
    stay_awake: true,
    show_touches: false,
  };
  const valid = message("launch_scrcpy", { request, path_grant: pathGrant });
  assert.equal(hook(valid), valid);
  assert.equal(
    hook(
      message("launch_scrcpy", {
        request: { ...request, video_encoder: "bad encoder" },
      }),
    ).cmd,
    blockedCommand,
  );
  assert.equal(
    hook(
      message("launch_scrcpy", {
        request: { ...request, record_path: "C:/arbitrary/output.mp4" },
      }),
    ).cmd,
    blockedCommand,
  );
});

test("validates the target for scrcpy capability probes", () => {
  const valid = message("scrcpy_capabilities", { target });
  const invalid = message("scrcpy_capabilities", {
    target: { ...target, transport_kind: "invented" },
  });
  assert.equal(hook(valid), valid);
  assert.equal(hook(invalid).cmd, blockedCommand);
});

test("allows bounded multiline Logcat exports", () => {
  const valid = message("save_logcat_export", {
    path_grant: pathGrant,
    contents: "first line\nsecond line\n",
  });
  assert.equal(hook(valid), valid);
});

test("allows bounded recovery baseline export and read-only inspection", () => {
  const exportMessage = message("export_recovery_baseline", {
    target,
    userId: 0,
    actions: [{ package: "com.example.app", kind: "disable" }],
    pack: { id: "pixel-safe", revision: 4 },
    path_grant: pathGrant,
  });
  const inspectMessage = message("inspect_recovery_baseline", {
    target,
    path_grant: pathGrant,
  });
  assert.equal(hook(exportMessage), exportMessage);
  assert.equal(hook(inspectMessage), inspectMessage);

  const invalid = message("export_recovery_baseline", {
    target,
    userId: 0,
    actions: [{ package: "com.example.app", kind: "shell" }],
    pack: null,
    path_grant: pathGrant,
  });
  assert.equal(hook(invalid).cmd, blockedCommand);
});

test("allows scoped package exports and rejects malformed Android users", () => {
  for (const cmd of ["export_package_apks", "backup_package"]) {
    const valid = message(cmd, {
      target,
      package: "com.example.app",
      userId: 10,
      path_grant: pathGrant,
      operation_id: `${cmd}-123`,
      on_event: 99,
    });
    assert.equal(hook(valid), valid);
    assert.equal(
      hook(message(cmd, { ...valid.payload, userId: -1 })).cmd,
      blockedCommand,
    );
  }
});

test("requires explicit privacy confirmation for bugreport capture", () => {
  const valid = message("capture_bugreport", {
    target,
    path_grant: pathGrant,
    privacy_confirmed: true,
    operation_id: "bugreport-123",
    on_event: 99,
  });
  assert.equal(hook(valid), valid);
  assert.equal(
    hook(
      message("capture_bugreport", {
        ...valid.payload,
        privacy_confirmed: false,
      }),
    ).cmd,
    blockedCommand,
  );
});

test("allows only bounded native dialog purposes and file names", () => {
  const valid = message("select_host_path", {
    purpose: "screenshot_save",
    suggested_name: "capture.png",
  });
  assert.equal(hook(valid), valid);
  const recovery = message("select_host_path", {
    purpose: "recovery_baseline_open",
    suggested_name: null,
  });
  assert.equal(hook(recovery), recovery);
  const packageExport = message("select_host_path", {
    purpose: "package_export_save",
    suggested_name: "com.example.app.apks.zip",
  });
  assert.equal(hook(packageExport), packageExport);
  const bugreport = message("select_host_path", {
    purpose: "bugreport_save",
    suggested_name: "droidsmith-bugreport-2026-07-15.zip",
  });
  assert.equal(hook(bugreport), bugreport);
  const recording = message("select_host_path", {
    purpose: "scrcpy_record_save",
    suggested_name: "droidsmith-recording-2026-07-15.mp4",
  });
  assert.equal(hook(recording), recording);

  for (const payload of [
    { purpose: "arbitrary_write", suggested_name: "capture.png" },
    { purpose: "screenshot_save", suggested_name: "../capture.png" },
  ]) {
    assert.equal(
      hook(message("select_host_path", payload)).cmd,
      blockedCommand,
    );
  }
});
