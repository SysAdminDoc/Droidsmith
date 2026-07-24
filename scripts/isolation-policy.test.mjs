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
const languageContract = JSON.parse(
  await readFile(new URL("../language-contract.json", import.meta.url), "utf8"),
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
    /tauri_specta::collect_commands!\[([\s\S]*?)\]\)/u,
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

test("validates captured display-control recovery argv", () => {
  const valid = message("apply_action", {
    plan: {
      request: {
        context: {
          device_control_restore_argv: ["wm", "density", "480"],
        },
      },
    },
  });
  assert.equal(hook(valid), valid);

  const injected = message("apply_action", {
    plan: {
      request: {
        context: {
          device_control_restore_argv: ["wm", "density", "480\nreboot"],
        },
      },
    },
  });
  assert.equal(hook(injected).cmd, blockedCommand);
});

test("passes a package action whose shell_argv is empty (debloat/disable)", () => {
  // Non-shell package mutations (disable/enable/uninstall/archive) always carry
  // an empty context.shell_argv; the policy must not reject that empty array.
  const disable = message("apply_action", {
    plan: {
      request: {
        serial: target.serial,
        target,
        package: "com.samsung.android.app.tips",
        kind: "disable",
        user_id: 0,
        pack_context: {
          pack_id: "qa-pack",
          revision: 1,
          provenance_source: "bundled",
          provenance_license: "MIT",
          compatibility_status: "compatible",
          override_accepted: false,
        },
        context: {
          confirmation_source: "debloat_preview",
          shell_argv: [],
          device_control_restore_argv: [],
        },
      },
      args: [
        "pm",
        "disable-user",
        "--user",
        "0",
        "com.samsung.android.app.tips",
      ],
      incident_id: "op-abc123",
      description: "Disable com.samsung.android.app.tips for user 0",
      before_state: "installed_enabled",
    },
  });
  assert.equal(hook(disable), disable);

  // A real (non-empty) shell_argv is still held to the argv-injection contract.
  const injected = message("apply_action", {
    plan: {
      request: {
        context: { shell_argv: ["settings", "put", "x\nreboot"] },
      },
    },
  });
  assert.equal(hook(injected).cmd, blockedCommand);
});

test("opens only the backend-resolved diagnostics directory", () => {
  const valid = message("reveal_diagnostics_directory", {});
  assert.equal(hook(valid), valid);
  assert.equal(
    hook(
      message("reveal_diagnostics_directory", {
        path: "C:/Users/QA/Documents",
      }),
    ).cmd,
    blockedCommand,
  );
});

test("accepts exactly every shipped settings language", () => {
  const supported = languageContract.languages.map((language) => language.code);
  assert.deepEqual(supported, ["de", "en", "es", "ru", "zh"]);
  for (const language of supported) {
    const valid = message("set_settings_language", { language });
    assert.equal(hook(valid), valid);
  }
  for (const language of ["", "fr", "EN", "../de", 7, null]) {
    assert.equal(
      hook(message("set_settings_language", { language })).cmd,
      blockedCommand,
    );
  }
});

test("validates portable settings import previews and apply modes", () => {
  const preview = message("preview_settings_import", {
    path_grant: pathGrant,
  });
  const merge = message("apply_settings_import", {
    importId: pathGrant,
    mode: "merge",
  });
  const replace = message("apply_settings_import", {
    importId: pathGrant,
    mode: "replace",
  });
  const restore = message("restore_settings_import_backup", {});
  for (const valid of [preview, merge, replace, restore]) {
    assert.equal(hook(valid), valid);
  }

  for (const invalid of [
    message("preview_settings_import", { path_grant: "C:/settings.json" }),
    message("apply_settings_import", {
      importId: "not-a-preview-id",
      mode: "merge",
    }),
    message("apply_settings_import", {
      importId: pathGrant,
      mode: "overwrite",
    }),
  ]) {
    assert.equal(hook(invalid).cmd, blockedCommand);
  }
});

test("permits only explicitly confirmed structured file mutations", () => {
  const valid = message("apply_remote_file_mutation", {
    target,
    request: {
      kind: "rename",
      source_path: "/sdcard/Download/Résumé final.txt",
      destination_path: "/sdcard/Download/Résumé archived.txt",
    },
    confirmed: true,
  });
  assert.equal(hook(valid), valid);

  for (const invalid of [
    { ...valid.payload, confirmed: false },
    {
      ...valid.payload,
      request: {
        ...valid.payload.request,
        source_path: "/sdcard/../data/secret",
      },
    },
    {
      ...valid.payload,
      request: {
        kind: "delete_file",
        source_path: "/sdcard/Download/report.txt",
        destination_path: "/sdcard/Download/other.txt",
      },
    },
  ]) {
    assert.equal(
      hook(message("apply_remote_file_mutation", invalid)).cmd,
      blockedCommand,
    );
  }
});

test("requires confirmation for a native-grant file push", () => {
  const payload = {
    target,
    path_grant: pathGrant,
    remote_path: "/sdcard/Download/Résumé final.txt",
    confirmed: true,
    operation_id: "push-123",
    on_event: 99,
  };
  const valid = message("push_file", payload);
  assert.equal(hook(valid), valid);
  assert.equal(
    hook(message("push_file", { ...payload, confirmed: false })).cmd,
    blockedCommand,
  );
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

test("bounds lazy package metadata requests to one validated package", () => {
  const valid = message("get_package_metadata", {
    target,
    package: "com.example.app",
    userId: 0,
  });
  assert.equal(hook(valid), valid);
  assert.equal(
    hook(
      message("get_package_metadata", {
        ...valid.payload,
        package: "../base.apk",
      }),
    ).cmd,
    blockedCommand,
  );
  assert.equal(
    hook(
      message("get_package_metadata", {
        ...valid.payload,
        packages: ["com.other"],
      }),
    ).cmd,
    blockedCommand,
  );
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

test("allows scoped profile preview and validates current schema exports", () => {
  const inspectMessage = message("inspect_profile", {
    target,
    path_grant: pathGrant,
  });
  const profile = {
    name: "QA setup",
    version: "2",
    description: "Reviewed package state",
    device: {
      require_serial_prefix: "R58",
      require_manufacturer: "Google",
      require_model: "Pixel 9",
      require_android_min: 34,
      require_android_max: 36,
    },
    user: { mode: "explicit", id: 10 },
    actions: [{ kind: "disable", package: "com.example.app" }],
  };
  const saveMessage = message("save_profile", {
    path_grant: pathGrant,
    profile,
  });
  assert.equal(hook(inspectMessage), inspectMessage);
  assert.equal(hook(saveMessage), saveMessage);

  for (const invalidProfile of [
    { ...profile, version: "1" },
    { ...profile, user: { mode: "current", id: 10 } },
    { ...profile, actions: [{ kind: "shell", package: "com.example.app" }] },
    {
      ...profile,
      device: {
        ...profile.device,
        require_android_min: 36,
        require_android_max: 34,
      },
    },
  ]) {
    assert.equal(
      hook(
        message("save_profile", {
          path_grant: pathGrant,
          profile: invalidProfile,
        }),
      ).cmd,
      blockedCommand,
    );
  }
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

test("gates Perfetto capture to fixed presets and explicit local privacy review", () => {
  const capabilities = message("perfetto_capabilities", { target });
  assert.equal(hook(capabilities), capabilities);
  const valid = message("capture_perfetto_trace", {
    target,
    path_grant: pathGrant,
    presetId: "ui_rendering",
    privacy_confirmed: true,
    operation_id: "perfetto-123",
    on_event: 99,
  });
  assert.equal(hook(valid), valid);
  for (const invalid of [
    { ...valid.payload, privacy_confirmed: false },
    { ...valid.payload, presetId: "../../custom" },
    { ...valid.payload, path_grant: "not-a-grant" },
  ]) {
    assert.equal(
      hook(message("capture_perfetto_trace", invalid)).cmd,
      blockedCommand,
    );
  }

  const open = message("open_artifact_with", {
    path: "C:/Users/QA/Desktop/trace.perfetto-trace",
  });
  assert.equal(hook(open), open);
  assert.equal(
    hook(message("open_artifact_with", { path: "../escape" })).cmd,
    blockedCommand,
  );
});

test("persists bounded Logcat query presets and rejects catastrophic regex", () => {
  const query = {
    id: "crash-watch",
    name: "Crashes",
    tagFilter: "ActivityManager",
    messageFilter: "FATAL EXCEPTION|ANR in",
    pidFilter: "",
    minLevel: "E",
    maxAgeSeconds: 3600,
    useRegex: true,
    negateTag: false,
    negateMessage: false,
    negatePid: false,
  };
  const list = message("list_logcat_queries", { deviceIdentity: null });
  assert.equal(hook(list), list);
  const listDevice = message("list_logcat_queries", {
    deviceIdentity: "R58M12345",
  });
  assert.equal(hook(listDevice), listDevice);

  const saveGlobal = message("save_logcat_queries", {
    scope: "global",
    deviceIdentity: null,
    queries: [query],
  });
  assert.equal(hook(saveGlobal), saveGlobal);
  const saveDevice = message("save_logcat_queries", {
    scope: "device",
    deviceIdentity: "R58M12345",
    queries: [query],
  });
  assert.equal(hook(saveDevice), saveDevice);
  const clear = message("save_logcat_queries", {
    scope: "global",
    deviceIdentity: null,
    queries: [],
  });
  assert.equal(hook(clear), clear);

  for (const invalid of [
    { scope: "device", deviceIdentity: null, queries: [query] },
    { scope: "planet", deviceIdentity: null, queries: [query] },
    {
      scope: "global",
      deviceIdentity: null,
      queries: [{ ...query, messageFilter: "(a+)+" }],
    },
    {
      scope: "global",
      deviceIdentity: null,
      queries: [{ ...query, messageFilter: "(\\w)\\1" }],
    },
    {
      scope: "global",
      deviceIdentity: null,
      queries: [{ ...query, id: "bad id" }],
    },
    {
      scope: "global",
      deviceIdentity: null,
      queries: [query, { ...query }],
    },
  ]) {
    assert.equal(
      hook(message("save_logcat_queries", invalid)).cmd,
      blockedCommand,
    );
  }
});

test("allows read-only layout capture and bounded hierarchy export", () => {
  const capture = message("capture_layout", { target });
  assert.equal(hook(capture), capture);
  const save = message("save_layout_export", {
    path_grant: pathGrant,
    contents: "<hierarchy></hierarchy>\n",
  });
  assert.equal(hook(save), save);

  const malformedTarget = hook(
    message("capture_layout", {
      target: { ...target, connection_generation: 0 },
    }),
  );
  const badGrant = hook(
    message("save_layout_export", {
      path_grant: "not-a-grant",
      contents: "<hierarchy/>",
    }),
  );
  assert.equal(malformedTarget.cmd, blockedCommand);
  assert.equal(badGrant.cmd, blockedCommand);
});

test("scopes pack import, pack export, and offline APK analysis", () => {
  const analyze = message("analyze_apk", { path_grant: pathGrant });
  const importPack = message("import_pack", {
    path_grant: pathGrant,
    expectedSha256: "a".repeat(64),
  });
  const removePack = message("remove_imported_pack", {
    packId: "qa-debloat",
  });
  const exportPack = message("export_device_pack", {
    target,
    userId: 10,
    path_grant: pathGrant,
  });
  for (const valid of [analyze, importPack, removePack, exportPack]) {
    assert.equal(hook(valid), valid);
  }

  for (const invalid of [
    message("import_pack", {
      path_grant: pathGrant,
      expectedSha256: "not-a-sha256",
    }),
    message("remove_imported_pack", { packId: "../escape" }),
    message("export_device_pack", {
      target,
      userId: -1,
      path_grant: pathGrant,
    }),
  ]) {
    assert.equal(hook(invalid).cmd, blockedCommand);
  }
});

test("validates device settings, wireless history, and gnirehtet controls", () => {
  const validMessages = [
    message("list_device_settings", { target }),
    message("put_device_setting", {
      target,
      settingId: "window_animation_scale",
      value: "0.5",
    }),
    message("list_running_services", {
      target,
      package: "com.example.app",
    }),
    message("disconnect_device", { target }),
    message("observe_device_fingerprint", { target }),
    message("forget_wireless_endpoint", { host: "pixel.local", port: 5555 }),
    message("set_wireless_auto_reconnect", { enabled: true }),
    message("start_gnirehtet", { target }),
    message("find_gnirehtet_session", { target }),
    message("gnirehtet_session_status", { session_id: 7 }),
    message("stop_gnirehtet", { session_id: 7 }),
  ];
  for (const valid of validMessages) assert.equal(hook(valid), valid);

  for (const invalid of [
    message("put_device_setting", {
      target,
      settingId: "arbitrary_secure_key",
      value: "1",
    }),
    message("forget_wireless_endpoint", { host: "-a", port: 5555 }),
    message("set_wireless_auto_reconnect", { enabled: "yes" }),
    message("stop_gnirehtet", { session_id: 0 }),
  ]) {
    assert.equal(hook(invalid).cmd, blockedCommand);
  }
});

test("accepts only supported absolute OS-dropped Android packages", () => {
  const valid = message("grant_dropped_path", {
    path: "C:/Users/QA/Downloads/app.apks",
  });
  assert.equal(hook(valid), valid);
  for (const path of [
    "relative/app.apk",
    "C:/Users/QA/../secret.apk",
    "C:/Users/QA/Downloads/readme.txt",
  ]) {
    assert.equal(
      hook(message("grant_dropped_path", { path })).cmd,
      blockedCommand,
    );
  }
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
  const profileOpen = message("select_host_path", {
    purpose: "profile_open",
    suggested_name: null,
  });
  const packImport = message("select_host_path", {
    purpose: "pack_import_open",
    suggested_name: null,
  });
  const packExport = message("select_host_path", {
    purpose: "pack_export_save",
    suggested_name: "device-debloat.yaml",
  });
  const apkAnalyze = message("select_host_path", {
    purpose: "apk_analyze_open",
    suggested_name: null,
  });
  const settingsImport = message("select_host_path", {
    purpose: "settings_import",
    suggested_name: null,
  });
  for (const valid of [
    profileOpen,
    packImport,
    packExport,
    apkAnalyze,
    settingsImport,
  ]) {
    assert.equal(hook(valid), valid);
  }

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
