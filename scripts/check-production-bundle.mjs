import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { env, execPath, platform, stdout } from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(repoRoot, "src-tauri");
const packageJson = readJson(path.join(repoRoot, "package.json"));
const tauriConfig = readJson(path.join(tauriDir, "tauri.conf.json"));
const npmBin = "npm";

cleanGeneratedOutputs();
runNpmScript("build");
verifyFrontendDist();
run(execPath, [path.join(repoRoot, "scripts", "check-tauri-resources.mjs")]);
verifyBundleMetadata();
verifyThirdPartyNotices();
runNpmScript("tauri:build");
verifyArtifacts();

stdout.write("Production bundle smoke OK\n");

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, "utf8"));
}

function runNpmScript(scriptName) {
  if (env.npm_execpath && fs.existsSync(env.npm_execpath)) {
    run(execPath, [env.npm_execpath, "run", scriptName]);
    return;
  }

  if (platform === "win32") {
    run("cmd.exe", ["/d", "/s", "/c", `${npmBin} run ${scriptName}`]);
    return;
  }

  run(npmBin, ["run", scriptName]);
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    shell: options.shell ?? false,
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited with ${result.status}`);
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function assertFile(filePath, label) {
  const stat = fs.statSync(filePath, { throwIfNoEntry: false });
  assert(stat?.isFile(), `Missing ${label}: ${path.relative(repoRoot, filePath)}`);
  assert(stat.size > 0, `${label} is empty: ${path.relative(repoRoot, filePath)}`);
}

function cleanGeneratedOutputs() {
  const releaseDir = path.join(tauriDir, "target", "release");
  const executableName = platform === "win32" ? "droidsmith.exe" : "droidsmith";
  for (const target of [
    path.join(repoRoot, "dist", "assets"),
    path.join(repoRoot, "dist", "index.html"),
    path.join(releaseDir, "bundle"),
    path.join(releaseDir, executableName),
  ]) {
    const fullPath = path.resolve(target);
    assert(
      fullPath === repoRoot || fullPath.startsWith(`${repoRoot}${path.sep}`),
      `Refusing to remove path outside repo: ${target}`,
    );
    fs.rmSync(fullPath, { recursive: true, force: true });
  }
}

function verifyFrontendDist() {
  const distDir = path.join(repoRoot, "dist");
  const assetsDir = path.join(distDir, "assets");
  assertFile(path.join(distDir, "index.html"), "frontend index");
  assert(fs.statSync(assetsDir, { throwIfNoEntry: false })?.isDirectory(), "Missing dist/assets");

  const assets = fs.readdirSync(assetsDir);
  assert(assets.some((name) => name.endsWith(".js")), "Missing built JS asset");
  assert(assets.some((name) => name.endsWith(".css")), "Missing built CSS asset");
}

function verifyBundleMetadata() {
  assert(packageJson.version === tauriConfig.version, "package.json and tauri.conf.json versions differ");
  assert(tauriConfig.productName === "Droidsmith", "Unexpected Tauri productName");
  assert(tauriConfig.identifier === "com.droidsmith.app", "Unexpected Tauri identifier");
  assert(tauriConfig.bundle?.active === true, "Tauri bundling must stay active");
  assert(tauriConfig.bundle?.targets === "all", "Tauri bundle targets must be all");
  assert(tauriConfig.bundle?.publisher, "Tauri bundle publisher is required");

  for (const icon of tauriConfig.bundle?.icon ?? []) {
    assertFile(path.join(tauriDir, icon), `bundle icon ${icon}`);
  }

  const externalBins = tauriConfig.bundle?.externalBin;
  if (externalBins === undefined) {
    stdout.write("bundle.externalBin not configured; platform-tools sidecars remain blocked\n");
    return;
  }

  assert(Array.isArray(externalBins), "bundle.externalBin must be an array when configured");
  for (const bin of externalBins) {
    const resolved = path.resolve(tauriDir, bin);
    const parent = path.dirname(resolved);
    const stem = path.basename(resolved);
    const candidates = fs
      .readdirSync(parent, { withFileTypes: true })
      .filter((entry) => entry.isFile() && (entry.name === stem || entry.name.startsWith(`${stem}-`)));
    assert(candidates.length > 0, `Missing sidecar artifact for externalBin entry ${bin}`);
  }
}

function verifyThirdPartyNotices() {
  const manifest = readJson(path.join(repoRoot, "third-party-notices.json"));
  assert(manifest.schemaVersion === 1, "third-party-notices.json schemaVersion must be 1");
  assert(Array.isArray(manifest.notices), "third-party-notices.json notices must be an array");

  const required = new Map([
    ["android-platform-tools-adb", "Apache-2.0"],
    ["android-platform-tools-fastboot", "Apache-2.0"],
    ["scrcpy", "Apache-2.0"],
    ["droidsmith-packs", "MIT"],
    ["droidsmith-quirks", "MIT"],
    ["universal-android-debloater-ng", "GPL-3.0"],
  ]);
  const byId = new Map(manifest.notices.map((notice) => [notice.id, notice]));

  for (const [id, license] of required) {
    const notice = byId.get(id);
    assert(notice, `Missing third-party notice entry: ${id}`);
    assert(notice.license === license, `Unexpected license for ${id}: ${notice.license}`);
    assert(Boolean(notice.source), `Missing source URL/path for ${id}`);
    assert(Boolean(notice.usage), `Missing usage note for ${id}`);
  }
}

function verifyArtifacts() {
  const releaseDir = path.join(tauriDir, "target", "release");
  if (platform === "win32") {
    assertFile(path.join(releaseDir, "droidsmith.exe"), "release executable");
    assertFile(
      path.join(releaseDir, "bundle", "msi", `Droidsmith_${packageJson.version}_x64_en-US.msi`),
      "MSI installer",
    );
    assertFile(
      path.join(releaseDir, "bundle", "nsis", `Droidsmith_${packageJson.version}_x64-setup.exe`),
      "NSIS installer",
    );
    return;
  }

  assertFile(path.join(releaseDir, "droidsmith"), "release executable");
  const bundleDir = path.join(releaseDir, "bundle");
  const bundledFiles = fs.existsSync(bundleDir) ? walkFiles(bundleDir) : [];
  assert(bundledFiles.length > 0, "Missing platform bundle artifacts");
  for (const filePath of bundledFiles) {
    assertFile(filePath, `bundle artifact ${path.relative(bundleDir, filePath)}`);
  }
}

function walkFiles(root) {
  const out = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const fullPath = path.join(root, entry.name);
    if (entry.isDirectory()) {
      out.push(...walkFiles(fullPath));
    } else if (entry.isFile()) {
      out.push(fullPath);
    }
  }
  return out;
}
