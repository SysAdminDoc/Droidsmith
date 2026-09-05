import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const screenshotNames = [
  "droidsmith-overview.png",
  "droidsmith-apps.png",
  "droidsmith-debloat.png",
  "droidsmith-profiles.png",
  "droidsmith-mirror.png",
  "droidsmith-logcat.png",
  "droidsmith-apk-analyzer.png",
];

function readPngSize(filePath) {
  const bytes = fs.readFileSync(filePath);
  assert.equal(bytes.subarray(1, 4).toString("ascii"), "PNG");
  return {
    width: bytes.readUInt32BE(16),
    height: bytes.readUInt32BE(20),
    byteLength: bytes.length,
  };
}

test("README screenshots are full-size current product captures", () => {
  const readme = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  for (const name of screenshotNames) {
    const relativePath = `docs/screenshots/${name}`;
    const size = readPngSize(path.join(repoRoot, relativePath));
    assert.deepEqual(
      { width: size.width, height: size.height },
      { width: 1366, height: 900 },
      `${name} must retain the reviewed capture viewport`,
    );
    assert.ok(size.byteLength > 50_000, `${name} looks unexpectedly empty`);
    assert.ok(
      readme.includes(relativePath),
      `${name} is missing from README.md`,
    );
  }
});

test("repository social preview has the GitHub card dimensions", () => {
  const filePath = path.join(repoRoot, ".github", "social-preview.png");
  const size = readPngSize(filePath);
  assert.deepEqual(
    { width: size.width, height: size.height },
    { width: 1280, height: 640 },
  );
  assert.ok(
    size.byteLength > 100_000,
    "social preview looks unexpectedly empty",
  );
  const readme = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  assert.ok(readme.includes(".github/social-preview.png"));
});

test("public version references match package.json", () => {
  const version = JSON.parse(
    fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"),
  ).version;
  const readme = fs.readFileSync(path.join(repoRoot, "README.md"), "utf8");
  assert.ok(readme.includes(`version-${version}-`));
  assert.ok(readme.includes(`/releases/tag/v${version}`));
});
