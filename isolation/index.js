/* global window */

(() => {
  "use strict";

  const BLOCKED_COMMAND = "__droidsmith_isolation_rejected__";
  const MAX_IDENTIFIER_LENGTH = 512;
  const MAX_STRING_LENGTH = 64 * 1024;
  const MAX_LOGCAT_EXPORT_LENGTH = 8 * 1024 * 1024;

  const READ_ONLY_COMMANDS = new Set([
    "heartbeat",
    "run_host_doctor",
    "list_devices",
    "preview_diagnostics",
    "list_wireless_services",
    "get_device_info",
    "list_packages",
    "preflight_package_backup",
    "list_users",
    "list_packs",
    "plan_pack",
    "plan_action",
    "plan_shell_action",
    "journal_list",
    "list_network_connections",
    "list_permissions",
    "list_processes",
    "list_remote_files",
    "plan_remote_file_mutation",
    "locate_scrcpy",
    "scrcpy_session_status",
    "locate_fastboot",
    "list_fastboot_devices",
    "fastboot_getvar",
    "explain_failure",
  ]);

  const SENSITIVE_COMMANDS = Object.freeze({
    watch_devices: [[], ["operation_id", "on_event"]],
    recover_adb: [["confirmed", "operation_id", "on_event"], []],
    select_host_path: [["purpose", "suggested_name"], []],
    save_diagnostics: [["path_grant"], []],
    wipe_diagnostics: [["confirmed"], []],
    pair_wireless: [["request"], []],
    connect_wireless: [["request"], []],
    apply_action: [["plan"], []],
    export_recovery_baseline: [
      ["target", "userId", "actions", "pack", "path_grant"],
      [],
    ],
    inspect_recovery_baseline: [["target", "path_grant"], []],
    inspect_profile: [["target", "path_grant"], []],
    save_profile: [["path_grant", "profile"], []],
    journal_undo: [["target", "entry_id"], []],
    backup_package: [
      ["target", "package", "userId", "path_grant", "operation_id", "on_event"],
      [],
    ],
    export_package_apks: [
      ["target", "package", "userId", "path_grant", "operation_id", "on_event"],
      [],
    ],
    capture_bugreport: [
      ["target", "path_grant", "privacy_confirmed", "operation_id", "on_event"],
      [],
    ],
    apply_remote_file_mutation: [["target", "request", "confirmed"], []],
    push_file: [
      [
        "target",
        "path_grant",
        "remote_path",
        "confirmed",
        "operation_id",
        "on_event",
      ],
      [],
    ],
    pull_file: [
      ["target", "remote_path", "path_grant", "operation_id", "on_event"],
      [],
    ],
    set_permission: [
      ["target", "package", "permission", "grant", "userId"],
      [],
    ],
    get_package_metadata: [["target", "package", "userId"], []],
    take_screenshot: [["target", "path_grant"], []],
    scrcpy_capabilities: [["target"], []],
    launch_scrcpy: [["request"], ["path_grant"]],
    stop_scrcpy: [["session_id"], []],
    shell_run: [["target", "argv", "operation_id", "on_event"], []],
    stream_logcat: [["target", "operation_id", "on_event"], []],
    cancel_operation: [["operation_id"], []],
    save_logcat_export: [["path_grant", "contents"], []],
    apply_device_control: [["target", "argv"], []],
    install_apk: [
      ["target", "path_grant", "options", "operation_id", "on_event"],
      [],
    ],
    extract_apk: [
      ["target", "remote_path", "path_grant", "operation_id", "on_event"],
      [],
    ],
  });

  const TARGET_KEYS = new Set([
    "serial",
    "transport_id",
    "connection_generation",
    "transport_kind",
    "untrusted_transport_override",
    "model",
    "product",
    "device",
    "build_fingerprint",
  ]);
  const PATH_PURPOSES = new Set([
    "diagnostics_save",
    "bugreport_save",
    "scrcpy_record_save",
    "logcat_save",
    "package_export_save",
    "backup_save",
    "screenshot_save",
    "pull_save",
    "extract_apk_save",
    "recovery_baseline_save",
    "profile_save",
    "push_open",
    "install_open",
    "recovery_baseline_open",
    "profile_open",
  ]);

  function isRecord(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value);
  }

  function hasOwn(value, key) {
    return Object.prototype.hasOwnProperty.call(value, key);
  }

  function reject(code) {
    throw new Error(code);
  }

  function hasControlCharacter(value) {
    for (const character of value) {
      const code = character.codePointAt(0);
      if (code < 32 || code === 127) return true;
    }
    return false;
  }

  function ensureIdentifier(value, code) {
    if (
      typeof value !== "string" ||
      value.length === 0 ||
      value.length > MAX_IDENTIFIER_LENGTH ||
      hasControlCharacter(value)
    ) {
      reject(code);
    }
  }

  function ensureInteger(value, code, minimum = 0) {
    if (!Number.isSafeInteger(value) || value < minimum) reject(code);
  }

  function validateKeys(value, required, optional, code) {
    if (!isRecord(value)) reject(`${code}_not_object`);
    const allowed = new Set([...required, ...optional]);
    for (const key of Object.keys(value)) {
      if (!allowed.has(key)) reject(`${code}_unexpected_key`);
    }
    for (const key of required) {
      if (!hasOwn(value, key)) reject(`${code}_missing_key`);
    }
  }

  function validateTarget(target) {
    validateKeys(target, [...TARGET_KEYS], [], "target");
    ensureIdentifier(target.serial, "target_serial");
    if (target.serial.startsWith("-")) reject("target_serial");
    ensureInteger(target.connection_generation, "target_generation", 1);
    if (
      !["usb", "tls_wifi", "legacy_tcp", "unknown_tcp"].includes(
        target.transport_kind,
      )
    ) {
      reject("target_transport_kind");
    }
    if (typeof target.untrusted_transport_override !== "boolean") {
      reject("target_transport_override");
    }
    if (target.transport_id !== null) {
      ensureInteger(target.transport_id, "target_transport", 1);
    }
    for (const key of ["model", "product", "device", "build_fingerprint"]) {
      if (target[key] !== null) ensureIdentifier(target[key], `target_${key}`);
    }
  }

  function validateLocalPath(value) {
    ensureIdentifier(value, "local_path");
    const windowsDrive = /^[A-Za-z]:[\\/]/u.test(value);
    const windowsUnc = /^\\\\[^\\/]+[\\/][^\\/]+/u.test(value);
    const unix = value.startsWith("/");
    if (!windowsDrive && !windowsUnc && !unix) reject("local_path_relative");
    if (value.split(/[\\/]+/u).some((part) => part === "." || part === "..")) {
      reject("local_path_traversal");
    }
  }

  function validateRemotePath(value) {
    ensureIdentifier(value, "remote_path");
    if (!value.startsWith("/") || value.startsWith("-")) {
      reject("remote_path_relative");
    }
    if (value.split("/").some((part) => part === "." || part === "..")) {
      reject("remote_path_traversal");
    }
  }

  function validateArgv(value) {
    if (!Array.isArray(value) || value.length === 0 || value.length > 128) {
      reject("argv_shape");
    }
    for (const arg of value) {
      if (
        typeof arg !== "string" ||
        arg.length === 0 ||
        arg.length > 4096 ||
        arg.includes("\0") ||
        arg.includes("\r") ||
        arg.includes("\n")
      ) {
        reject("argv_value");
      }
    }
  }

  function validateWirelessHost(value, code) {
    ensureIdentifier(value, code);
    if (value.startsWith("-") || /[\s/\\]/u.test(value)) reject(code);
  }

  function validateNested(value, seen = new WeakSet(), depth = 0, key = "") {
    if (depth > 12) reject("payload_depth");
    if (typeof value === "string") {
      const limit =
        key === "contents" ? MAX_LOGCAT_EXPORT_LENGTH : MAX_STRING_LENGTH;
      if (value.length > limit || value.includes("\0"))
        reject("payload_string");
      if (key === "path_grant") {
        if (
          !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(
            value,
          )
        ) {
          reject("path_grant");
        }
      } else if (["local_path", "apk_path"].includes(key)) {
        validateLocalPath(value);
      } else if (
        ["remote_path", "source_path", "destination_path"].includes(key)
      ) {
        validateRemotePath(value);
      } else if (key === "package") {
        if (!/^[A-Za-z0-9_][A-Za-z0-9_.-]{0,254}$/u.test(value)) {
          reject("package_name");
        }
      } else if (key === "permission") {
        if (!/^[A-Za-z0-9_.]{1,255}$/u.test(value)) reject("permission_name");
      } else if (key === "operation_id") {
        if (!/^[a-z0-9][a-z0-9_.-]{0,127}$/u.test(value))
          reject("operation_id");
      }
      return;
    }
    if (value === null || typeof value !== "object") return;
    if (seen.has(value)) reject("payload_cycle");
    seen.add(value);
    if (Array.isArray(value)) {
      if (value.length > 4096) reject("payload_array");
      if (key === "argv" || key === "shell_argv") validateArgv(value);
      for (const entry of value) validateNested(entry, seen, depth + 1, "");
      return;
    }
    const keys = Object.keys(value);
    if (keys.length > 64) reject("payload_keys");
    if (key === "target") validateTarget(value);
    for (const nestedKey of keys) {
      if (nestedKey.length > 128) reject("payload_key");
      if (nestedKey === "userId" || nestedKey === "user_id") {
        ensureInteger(value[nestedKey], "user_id");
      }
      validateNested(value[nestedKey], seen, depth + 1, nestedKey);
    }
  }

  function validateRequest(command, request) {
    if (!isRecord(request)) reject(`${command}_request`);
    if (command === "pair_wireless") {
      validateKeys(
        request,
        ["host", "port", "pairing_code"],
        [],
        "pair_request",
      );
      validateWirelessHost(request.host, "pair_host");
      ensureInteger(request.port, "pair_port", 1);
      if (
        request.port > 65535 ||
        typeof request.pairing_code !== "string" ||
        !/^\d{6}$/u.test(request.pairing_code)
      ) {
        reject("pair_request_value");
      }
    } else if (command === "connect_wireless") {
      validateKeys(
        request,
        ["host", "port", "legacy_tcp"],
        [],
        "connect_request",
      );
      validateWirelessHost(request.host, "connect_host");
      ensureInteger(request.port, "connect_port", 1);
      if (request.port > 65535) reject("connect_port");
      if (typeof request.legacy_tcp !== "boolean") {
        reject("connect_legacy_tcp");
      }
    } else if (command === "launch_scrcpy") {
      validateKeys(
        request,
        [
          "serial",
          "target",
          "no_audio",
          "turn_screen_off",
          "stay_awake",
          "show_touches",
        ],
        [
          "max_size",
          "bit_rate",
          "keyboard_mode",
          "video_codec",
          "video_encoder",
        ],
        "scrcpy_request",
      );
      ensureIdentifier(request.serial, "scrcpy_serial");
      validateTarget(request.target);
      for (const flag of [
        "no_audio",
        "turn_screen_off",
        "stay_awake",
        "show_touches",
      ]) {
        if (typeof request[flag] !== "boolean") reject("scrcpy_flag");
      }
      if (request.serial !== request.target.serial) reject("scrcpy_target");
      if (
        request.video_codec != null &&
        !["h264", "h265", "av1", "vp8", "vp9"].includes(request.video_codec)
      ) {
        reject("scrcpy_video_codec");
      }
      if (
        request.video_encoder != null &&
        (typeof request.video_encoder !== "string" ||
          !/^[A-Za-z0-9_.-]{1,255}$/u.test(request.video_encoder))
      ) {
        reject("scrcpy_video_encoder");
      }
    }
  }

  function validateProfile(profile) {
    validateKeys(
      profile,
      ["name", "version", "actions"],
      ["description", "device", "user"],
      "profile",
    );
    ensureIdentifier(profile.name, "profile_name");
    if (profile.name.length > 200 || profile.version !== "2") {
      reject("profile_header");
    }
    if (
      !Array.isArray(profile.actions) ||
      profile.actions.length === 0 ||
      profile.actions.length > 2000
    ) {
      reject("profile_actions");
    }
    for (const action of profile.actions) {
      validateKeys(action, ["kind", "package"], ["note"], "profile_action");
      if (
        ![
          "disable",
          "enable",
          "uninstall_for_user",
          "restore_existing_for_user",
          "clear_data",
          "force_stop",
        ].includes(action.kind)
      ) {
        reject("profile_action_kind");
      }
    }
    if (profile.device !== undefined) {
      validateKeys(
        profile.device,
        [],
        [
          "require_serial_prefix",
          "require_manufacturer",
          "require_model",
          "require_android_min",
          "require_android_max",
        ],
        "profile_device",
      );
      for (const key of ["require_android_min", "require_android_max"]) {
        if (profile.device[key] !== undefined && profile.device[key] !== null) {
          ensureInteger(profile.device[key], `profile_device_${key}`, 1);
        }
      }
      if (
        profile.device.require_android_min != null &&
        profile.device.require_android_max != null &&
        profile.device.require_android_min > profile.device.require_android_max
      ) {
        reject("profile_device_android_range");
      }
    }
    if (profile.user !== undefined) {
      validateKeys(profile.user, [], ["mode", "id"], "profile_user");
      const mode = profile.user.mode ?? "owner";
      if (!["owner", "current", "explicit"].includes(mode)) {
        reject("profile_user_mode");
      }
      if (mode === "explicit") {
        ensureInteger(profile.user.id, "profile_user_id");
      } else if (profile.user.id !== undefined && profile.user.id !== null) {
        reject("profile_user_id");
      }
    }
  }

  function validateSpecific(command, payload) {
    if (command === "select_host_path") {
      if (!PATH_PURPOSES.has(payload.purpose)) reject("path_purpose");
      if (payload.suggested_name !== null) {
        ensureIdentifier(payload.suggested_name, "suggested_name");
        if (
          payload.suggested_name.length > 255 ||
          payload.suggested_name !== payload.suggested_name.trim() ||
          /[\\/]/u.test(payload.suggested_name) ||
          payload.suggested_name === "." ||
          payload.suggested_name === ".."
        ) {
          reject("suggested_name");
        }
      }
    }
    if (command === "recover_adb" || command === "wipe_diagnostics") {
      if (typeof payload.confirmed !== "boolean") reject("confirmation");
    }
    if (command === "capture_bugreport" && payload.privacy_confirmed !== true) {
      reject("bugreport_privacy_confirmation");
    }
    if (
      ["apply_remote_file_mutation", "push_file"].includes(command) &&
      payload.confirmed !== true
    ) {
      reject("file_mutation_confirmation");
    }
    if (command === "apply_remote_file_mutation") {
      validateKeys(
        payload.request,
        ["kind", "source_path"],
        ["destination_path"],
        "remote_file_request",
      );
      if (
        !["mkdir", "rename", "delete_file", "delete_directory"].includes(
          payload.request.kind,
        )
      ) {
        reject("remote_file_kind");
      }
      if (
        payload.request.kind === "rename" &&
        (typeof payload.request.destination_path !== "string" ||
          payload.request.destination_path.length === 0)
      ) {
        reject("remote_file_destination");
      }
      if (
        payload.request.kind !== "rename" &&
        payload.request.destination_path != null
      ) {
        reject("remote_file_destination");
      }
    }
    if (
      ["pair_wireless", "connect_wireless", "launch_scrcpy"].includes(command)
    ) {
      validateRequest(command, payload.request);
    }
    if (command === "apply_action" && !isRecord(payload.plan))
      reject("action_plan");
    if (command === "export_recovery_baseline") {
      if (
        !Array.isArray(payload.actions) ||
        payload.actions.length === 0 ||
        payload.actions.length > 2048
      ) {
        reject("baseline_actions");
      }
      for (const action of payload.actions) {
        validateKeys(action, ["package", "kind"], [], "baseline_action");
        if (
          ![
            "disable",
            "enable",
            "uninstall_for_user",
            "clear_data",
            "force_stop",
          ].includes(action.kind)
        ) {
          reject("baseline_action_kind");
        }
      }
      if (payload.pack !== null) {
        validateKeys(payload.pack, ["id", "revision"], [], "baseline_pack");
        ensureIdentifier(payload.pack.id, "baseline_pack_id");
        ensureInteger(payload.pack.revision, "baseline_pack_revision");
      }
    }
    if (command === "set_permission" && typeof payload.grant !== "boolean") {
      reject("permission_grant");
    }
    if (command === "install_apk") {
      validateKeys(
        payload.options,
        [],
        [
          "allow_downgrade",
          "bypass_low_target_sdk_block",
          "override_confirmed",
        ],
        "install_options",
      );
      for (const value of Object.values(payload.options)) {
        if (typeof value !== "boolean") reject("install_option");
      }
    }
    if (command === "save_profile") validateProfile(payload.profile);
    if (command === "journal_undo")
      ensureInteger(payload.entry_id, "entry_id", 1);
    if (command === "stop_scrcpy")
      ensureInteger(payload.session_id, "session_id", 1);
  }

  function validateCommand(message) {
    if (!isRecord(message)) reject("message");
    ensureIdentifier(message.cmd, "command");
    if (message.cmd.startsWith("plugin:")) return;
    if (READ_ONLY_COMMANDS.has(message.cmd)) return;
    const schema = SENSITIVE_COMMANDS[message.cmd];
    if (!schema) reject("command_not_allowed");
    validateKeys(
      message.payload,
      schema[0],
      schema[1],
      `${message.cmd}_payload`,
    );
    validateSpecific(message.cmd, message.payload);
    validateNested(message.payload);
  }

  function blockedMessage(message, code) {
    const blocked = Object.create(null);
    blocked.cmd = BLOCKED_COMMAND;
    blocked.callback = message?.callback;
    blocked.error = message?.error;
    blocked.options = message?.options;
    blocked.payload = Object.freeze({
      code: "ipc_policy_rejected",
      reason: code,
    });
    return blocked;
  }

  window.__TAURI_ISOLATION_HOOK__ = (message) => {
    try {
      validateCommand(message);
      return message;
    } catch (error) {
      const code = error instanceof Error ? error.message : "policy_error";
      return blockedMessage(message, code);
    }
  };
})();
