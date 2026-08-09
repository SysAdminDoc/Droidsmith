import assert from "node:assert/strict";
import test from "node:test";

import {
  collectCargoRuntimeComponents,
  collectNpmRuntimeComponents,
  generateProvenance,
  validateProvenance,
} from "./generate-provenance.mjs";

const packageJsonText = `${JSON.stringify({ name: "droidsmith", version: "1.2.3" })}\n`;
const packageLock = {
  name: "droidsmith",
  version: "1.2.3",
  lockfileVersion: 3,
  packages: {
    "": {
      name: "droidsmith",
      version: "1.2.3",
      dependencies: { "@scope/runtime": "1.0.0" },
      devDependencies: { "test-only": "9.0.0" },
    },
    "node_modules/@scope/runtime": {
      version: "1.0.0",
      integrity: "sha512-YWJj",
    },
    "node_modules/transitive": { version: "2.0.0", optional: true },
    "node_modules/test-only": { version: "9.0.0", dev: true },
  },
};
const packageLockText = `${JSON.stringify(packageLock, null, 2)}\n`;
const cargoManifestText = `[package]
name = "droidsmith"
version = "1.2.3"

[build-dependencies]
alpha = "1"

[target.'cfg(windows)'.dependencies]
beta-alias = { package = "actual-beta", version = "2" }

[dev-dependencies]
test-crate = "9"
`;
const cargoLockText = `version = 4

[[package]]
name = "droidsmith"
version = "1.2.3"
dependencies = [
 "actual-beta",
 "alpha",
 "test-crate",
]

[[package]]
name = "alpha"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
dependencies = [
 "shared",
]

[[package]]
name = "actual-beta"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"

[[package]]
name = "shared"
version = "3.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

[[package]]
name = "test-crate"
version = "9.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
`;

const inputs = {
  packageJsonText,
  packageLockText,
  cargoManifestText,
  cargoLockText,
  cargoMetadataText: `${JSON.stringify({
    packages: [
      {
        name: "droidsmith",
        version: "1.2.3",
        source: null,
        license: "MIT",
      },
      {
        name: "alpha",
        version: "1.0.0",
        source: "registry+https://github.com/rust-lang/crates.io-index",
        license: "MIT OR Apache-2.0",
      },
      {
        name: "actual-beta",
        version: "2.0.0",
        source: "registry+https://github.com/rust-lang/crates.io-index",
        license: "Apache-2.0",
      },
      {
        name: "shared",
        version: "3.0.0",
        source: "registry+https://github.com/rust-lang/crates.io-index",
        license: null,
      },
    ],
  })}\n`,
  noticesText: "{}\n",
};

test("npm inventory includes locked runtime packages and excludes dev-only packages", () => {
  const purls = collectNpmRuntimeComponents(packageLock).map(
    (component) => component.purl,
  );
  assert.deepEqual(purls, [
    "pkg:npm/%40scope/runtime@1.0.0",
    "pkg:npm/transitive@2.0.0",
  ]);
});

test("Cargo inventory walks runtime and build dependencies but not root dev dependencies", () => {
  const purls = collectCargoRuntimeComponents(
    cargoManifestText,
    cargoLockText,
  ).map((component) => component.purl);
  assert.deepEqual(purls, [
    "pkg:cargo/actual-beta@2.0.0",
    "pkg:cargo/alpha@1.0.0",
    "pkg:cargo/shared@3.0.0",
  ]);
});

test("provenance output is deterministic, parseable, and bound to every input", () => {
  const first = generateProvenance(inputs);
  const second = generateProvenance(inputs);
  assert.deepEqual(first, second);
  assert.doesNotThrow(() => validateProvenance(first, inputs));
  assert(
    first.sbom.components.every(
      (component) =>
        Array.isArray(component.licenses) && component.licenses.length > 0,
    ),
    "every SBOM component must carry a license marker",
  );
  assert.deepEqual(
    first.sbom.components.find((component) => component.name === "shared")
      ?.licenses,
    [{ license: { id: "NOASSERTION" } }],
    "undeclared Cargo licenses must carry an explicit unknown marker",
  );
  assert.match(first.checksumsText, /provenance\/SBOM\.cdx\.json/u);

  const changed = { ...inputs, noticesText: '{"changed":true}\n' };
  assert.throws(
    () => validateProvenance(first, changed),
    /SHA256SUMS differs from provenance inputs/u,
  );
});

test("provenance hashes are invariant to checkout newline style", () => {
  const windowsInputs = Object.fromEntries(
    Object.entries(inputs).map(([name, value]) => [
      name,
      value.replaceAll("\n", "\r\n"),
    ]),
  );
  assert.deepEqual(
    generateProvenance(windowsInputs),
    generateProvenance(inputs),
  );
});
