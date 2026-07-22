/* global window */

(() => {
  "use strict";

  const BLOCKED_COMMAND = "__droidsmith_isolation_rejected__";
  const MAX_IDENTIFIER_LENGTH = 512;
  const MAX_STRING_LENGTH = 64 * 1024;
  const MAX_LOGCAT_EXPORT_LENGTH = 8 * 1024 * 1024;
  // Mirrored from language-contract.json. The release-policy gate fails if
  // this allowlist, the renderer contract, or the Rust enum diverges.
  const SUPPORTED_LANGUAGE_CODES = new Set(["de", "en", "es", "ru", "zh"]);

  const READ_ONLY_COMMANDS = new Set([
    "heartbeat",
    "run_host_doctor",
    "list_devices",
    "preview_diagnostics",
    "list_wireless_services",
    "list_wireless_history",
    "get_device_info",
    "list_packages",
    "preflight_package_backup",
    "list_users",
    "list_packs",
    "plan_pack",
    "plan_action",
    "plan_action_batch",
    "plan_shell_action",
    "journal_list",
    "list_network_connections",
    "list_permissions",
    "list_processes",
    "list_remote_files",
    "plan_remote_file_mutation",
    "locate_scrcpy",
    "locate_gnirehtet",
    "scrcpy_session_status",
    "locate_fastboot",
    "list_fastboot_devices",
    "fastboot_getvar",
    "explain_failure",
    "has_settings_import_backup",
  ]);

  const SENSITIVE_COMMANDS = Object.freeze({
    watch_devices: [[], ["operation_id", "on_event"]],
    recover_adb: [["confirmed", "operation_id", "on_event"], []],
    select_host_path: [["purpose", "suggested_name"], []],
    reveal_in_folder: [["path"], []],
    reveal_diagnostics_directory: [[], []],
    initialize_settings: [["legacy"], []],
    set_settings_language: [["language"], []],
    get_settings_mirror_preset: [["deviceIdentity"], []],
    set_settings_mirror_preset: [["deviceIdentity", "preset"], []],
    reset_settings_mirror_preset: [["deviceIdentity"], []],
    reset_settings: [["scope"], []],
    export_settings: [["scope", "path_grant"], []],
    preview_settings_import: [["path_grant"], []],
    apply_settings_import: [["importId", "mode"], []],
    restore_settings_import_backup: [[], []],
    list_logcat_queries: [["deviceIdentity"], []],
    save_logcat_queries: [["scope", "deviceIdentity", "queries"], []],
    save_diagnostics: [["path_grant"], []],
    wipe_diagnostics: [["confirmed"], []],
    pair_wireless: [["request"], []],
    connect_wireless: [["request"], []],
    disconnect_device: [["target"], []],
    forget_wireless_endpoint: [["host", "port"], []],
    set_wireless_auto_reconnect: [["enabled"], []],
    observe_device_fingerprint: [["target"], []],
    apply_action: [["plan"], []],
    apply_action_batch: [["batch"], []],
    export_recovery_baseline: [
      ["target", "userId", "actions", "pack", "path_grant"],
      [],
    ],
    inspect_recovery_baseline: [["target", "path_grant"], []],
    inspect_profile: [["target", "path_grant"], []],
    save_profile: [["path_grant", "profile"], []],
    journal_undo: [["target", "entry_id"], []],
    journal_undo_batch: [["target", "batch_id"], []],
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
    list_device_settings: [["target"], []],
    put_device_setting: [["target", "settingId", "value"], []],
    list_running_services: [["target", "package"], []],
    take_screenshot: [["target", "path_grant"], []],
    scrcpy_capabilities: [["target"], []],
    launch_scrcpy: [["request"], ["path_grant"]],
    stop_scrcpy: [["session_id"], []],
    start_gnirehtet: [["target"], []],
    find_gnirehtet_session: [["target"], []],
    gnirehtet_session_status: [["session_id"], []],
    stop_gnirehtet: [["session_id"], []],
    shell_run: [["target", "argv", "operation_id", "on_event"], []],
    stream_logcat: [["target", "operation_id", "on_event"], []],
    cancel_operation: [["operation_id"], []],
    save_logcat_export: [["path_grant", "contents"], []],
    capture_layout: [["target"], []],
    save_layout_export: [["path_grant", "contents"], []],
    apply_device_control: [["target", "argv"], []],
    install_apk: [
      ["target", "path_grant", "options", "operation_id", "on_event"],
      [],
    ],
    extract_apk: [
      ["target", "remote_path", "path_grant", "operation_id", "on_event"],
      [],
    ],
    grant_dropped_path: [["path"], []],
    import_pack: [["path_grant", "expectedSha256"], []],
    remove_imported_pack: [["packId"], []],
    export_device_pack: [["target", "userId", "path_grant"], []],
    analyze_apk: [["path_grant"], []],
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
    "settings_export",
    "settings_import",
    "layout_export_save",
    "push_open",
    "install_open",
    "recovery_baseline_open",
    "profile_open",
    "pack_import_open",
    "pack_export_save",
    "apk_analyze_open",
  ]);

  const DEVICE_SETTING_IDS = new Set([
    "window_animation_scale",
    "transition_animation_scale",
    "animator_duration_scale",
    "screen_off_timeout",
    "font_scale",
    "stay_on_while_plugged_in",
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
      } else if (key === "batch_id") {
        if (!/^batch-[0-9A-Za-z-]{1,90}$/u.test(value)) reject("batch_id");
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

  function validateMirrorPreset(preset) {
    validateKeys(
      preset,
      [
        "maxSize",
        "bitRate",
        "noAudio",
        "recording",
        "keyboardMode",
        "videoCodec",
        "videoEncoder",
        "turnScreenOff",
        "stayAwake",
        "showTouches",
      ],
      [],
      "settings_mirror_preset",
    );
    if (
      typeof preset.maxSize !== "string" ||
      !/^\d{1,5}$/u.test(preset.maxSize)
    ) {
      reject("settings_max_size");
    }
    if (
      typeof preset.bitRate !== "string" ||
      !/^[0-9]+[kKmM]?$/u.test(preset.bitRate) ||
      preset.bitRate.length > 16
    ) {
      reject("settings_bit_rate");
    }
    if (
      !["default", "sdk", "uhid", "aoa", "disabled"].includes(
        preset.keyboardMode,
      )
    ) {
      reject("settings_keyboard_mode");
    }
    if (!["h264", "h265", "av1", "vp8", "vp9"].includes(preset.videoCodec)) {
      reject("settings_video_codec");
    }
    if (
      typeof preset.videoEncoder !== "string" ||
      preset.videoEncoder.length > 255 ||
      !/^[A-Za-z0-9_.-]*$/u.test(preset.videoEncoder)
    ) {
      reject("settings_video_encoder");
    }
    for (const key of [
      "noAudio",
      "recording",
      "turnScreenOff",
      "stayAwake",
      "showTouches",
    ]) {
      if (typeof preset[key] !== "boolean") reject(`settings_${key}`);
    }
  }

  function validateLegacySettings(legacy) {
    validateKeys(legacy, ["language"], ["mirrorPresets"], "legacy_settings");
    if (
      legacy.language !== null &&
      (typeof legacy.language !== "string" || legacy.language.length > 64)
    ) {
      reject("legacy_settings_language");
    }
    const presets = legacy.mirrorPresets ?? [];
    if (!Array.isArray(presets) || presets.length > 128) {
      reject("legacy_settings_presets");
    }
    for (const preset of presets) {
      validateKeys(
        preset,
        ["deviceIdentity", "rawValue"],
        [],
        "legacy_settings_preset",
      );
      ensureIdentifier(preset.deviceIdentity, "legacy_settings_device");
      if (
        typeof preset.rawValue !== "string" ||
        preset.rawValue.length > MAX_STRING_LENGTH
      ) {
        reject("legacy_settings_value");
      }
    }
  }

  function validateLinearRegex(pattern, code) {
    if (/\\[0-9]/u.test(pattern) || /\\k/u.test(pattern)) reject(code);
    if (/\(\?[=!]/u.test(pattern) || /\(\?<[=!]/u.test(pattern)) reject(code);
    if (/\)[*+]/u.test(pattern) || /\)\{/u.test(pattern)) reject(code);
    try {
      void new RegExp(pattern, "u");
    } catch {
      reject(code);
    }
  }

  function validateLogcatQuery(query) {
    validateKeys(
      query,
      ["id", "name", "minLevel"],
      [
        "tagFilter",
        "messageFilter",
        "pidFilter",
        "packageFilter",
        "processFilter",
        "maxAgeSeconds",
        "useRegex",
        "negateTag",
        "negateMessage",
        "negatePid",
        "negatePackage",
        "negateProcess",
      ],
      "logcat_query",
    );
    if (
      typeof query.id !== "string" ||
      !/^[A-Za-z0-9_-]{1,64}$/u.test(query.id)
    ) {
      reject("logcat_query_id");
    }
    if (
      typeof query.name !== "string" ||
      query.name.trim().length === 0 ||
      query.name.length > 80 ||
      hasControlCharacter(query.name)
    ) {
      reject("logcat_query_name");
    }
    if (!["V", "D", "I", "W", "E", "F"].includes(query.minLevel)) {
      reject("logcat_query_level");
    }
    const useRegex = query.useRegex === true;
    for (const [key, code] of [
      ["tagFilter", "logcat_query_tag"],
      ["messageFilter", "logcat_query_message"],
      ["packageFilter", "logcat_query_package"],
      ["processFilter", "logcat_query_process"],
    ]) {
      const value = query[key];
      if (value === undefined) continue;
      if (typeof value !== "string" || value.length > 256) reject(code);
      if (hasControlCharacter(value)) reject(code);
      if (useRegex && value.length > 0) validateLinearRegex(value, code);
    }
    if (query.pidFilter !== undefined) {
      if (
        typeof query.pidFilter !== "string" ||
        (query.pidFilter.length > 0 && !/^[0-9]{1,7}$/u.test(query.pidFilter))
      ) {
        reject("logcat_query_pid");
      }
    }
    if (query.maxAgeSeconds !== undefined && query.maxAgeSeconds !== null) {
      ensureInteger(query.maxAgeSeconds, "logcat_query_age", 1);
      if (query.maxAgeSeconds > 30 * 24 * 60 * 60) reject("logcat_query_age");
    }
    for (const key of [
      "useRegex",
      "negateTag",
      "negateMessage",
      "negatePid",
      "negatePackage",
      "negateProcess",
    ]) {
      if (query[key] !== undefined && typeof query[key] !== "boolean") {
        reject(`logcat_query_${key}`);
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
    if (command === "reveal_in_folder") {
      validateLocalPath(payload.path);
    }
    if (command === "grant_dropped_path") {
      validateLocalPath(payload.path);
      if (!/\.(?:apk|apks|xapk|apkm)$/iu.test(payload.path)) {
        reject("dropped_path_extension");
      }
    }
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
    if (command === "initialize_settings")
      validateLegacySettings(payload.legacy);
    if (command === "set_settings_language") {
      if (!SUPPORTED_LANGUAGE_CODES.has(payload.language))
        reject("settings_language");
    }
    if (
      [
        "get_settings_mirror_preset",
        "set_settings_mirror_preset",
        "reset_settings_mirror_preset",
      ].includes(command)
    ) {
      ensureIdentifier(payload.deviceIdentity, "settings_device");
    }
    if (command === "set_settings_mirror_preset") {
      validateMirrorPreset(payload.preset);
    }
    if (["reset_settings", "export_settings"].includes(command)) {
      if (!["all", "language", "mirror_presets"].includes(payload.scope)) {
        reject("settings_scope");
      }
    }
    if (command === "apply_settings_import") {
      if (
        !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(
          payload.importId,
        )
      ) {
        reject("settings_import_id");
      }
      if (!["merge", "replace"].includes(payload.mode)) {
        reject("settings_import_mode");
      }
    }
    if (command === "list_logcat_queries" && payload.deviceIdentity !== null) {
      ensureIdentifier(payload.deviceIdentity, "logcat_device");
    }
    if (command === "save_logcat_queries") {
      if (!["global", "device"].includes(payload.scope)) {
        reject("logcat_scope");
      }
      if (payload.scope === "device") {
        ensureIdentifier(payload.deviceIdentity, "logcat_device");
      } else if (payload.deviceIdentity !== null) {
        ensureIdentifier(payload.deviceIdentity, "logcat_device");
      }
      if (!Array.isArray(payload.queries) || payload.queries.length > 64) {
        reject("logcat_queries");
      }
      const ids = new Set();
      for (const query of payload.queries) {
        validateLogcatQuery(query);
        if (ids.has(query.id)) reject("logcat_query_duplicate");
        ids.add(query.id);
      }
    }
    if (command === "recover_adb" || command === "wipe_diagnostics") {
      if (typeof payload.confirmed !== "boolean") reject("confirmation");
    }
    if (command === "forget_wireless_endpoint") {
      validateWirelessHost(payload.host, "wireless_history_host");
      ensureInteger(payload.port, "wireless_history_port", 1);
      if (payload.port > 65535) reject("wireless_history_port");
    }
    if (
      command === "set_wireless_auto_reconnect" &&
      typeof payload.enabled !== "boolean"
    ) {
      reject("wireless_auto_reconnect");
    }
    if (command === "import_pack" && payload.expectedSha256 !== null) {
      if (
        typeof payload.expectedSha256 !== "string" ||
        !/^[0-9a-f]{64}$/iu.test(payload.expectedSha256)
      ) {
        reject("pack_sha256");
      }
    }
    if (command === "remove_imported_pack") {
      if (!/^[a-z0-9](?:[a-z0-9-]{1,62}[a-z0-9])$/u.test(payload.packId)) {
        reject("pack_id");
      }
    }
    if (command === "put_device_setting") {
      if (!DEVICE_SETTING_IDS.has(payload.settingId))
        reject("device_setting_id");
      if (
        typeof payload.value !== "string" ||
        payload.value.length === 0 ||
        payload.value.length > 32 ||
        payload.value !== payload.value.trim() ||
        !/^(?:\d+|\d*\.\d+)$/u.test(payload.value)
      ) {
        reject("device_setting_value");
      }
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
          "incremental",
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
    if (["gnirehtet_session_status", "stop_gnirehtet"].includes(command)) {
      ensureInteger(payload.session_id, "session_id", 1);
    }
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
