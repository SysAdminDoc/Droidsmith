import { describe, expect, it } from "vitest";

import type { Pack } from "../lib/tauri";
import {
  DEBLOAT_PRESETS,
  expandPackDependencies,
  packagesForPreset,
  summarizePackSelection,
} from "./debloatPack";

const pack: Pack = {
  id: "test-pack",
  revision: 1,
  name: "Test",
  version: "1",
  description: "Test pack",
  targets: {
    manufacturer: [],
    rom: [],
    model: [],
    build_fingerprint: [],
    android_min: null,
    android_max: null,
    user_scope: "any",
  },
  provenance: { source: "test", license: "MIT" },
  attribution: null,
  packages: [
    {
      id: "com.example.base",
      removal: "recommended",
      description: "Base",
      depends_on: [],
      needed_by: [],
      labels: [],
    },
    {
      id: "com.example.middle",
      removal: "advanced",
      description: "Middle",
      depends_on: ["com.example.base"],
      needed_by: [],
      labels: [],
    },
    {
      id: "com.example.top",
      removal: "expert",
      description: "Top",
      depends_on: ["com.example.middle"],
      needed_by: [],
      labels: [],
    },
    {
      id: "com.example.unsafe",
      removal: "unsafe",
      description: "Unsafe",
      depends_on: [],
      needed_by: [],
      labels: [],
    },
  ],
};

describe("expandPackDependencies", () => {
  it("adds transitive dependencies to the preview", () => {
    expect([...expandPackDependencies(pack, ["com.example.top"])]).toEqual([
      "com.example.base",
      "com.example.middle",
      "com.example.top",
    ]);
  });

  it("rejects renderer selections outside the signed pack revision", () => {
    expect(() => expandPackDependencies(pack, ["com.example.unknown"])).toThrow(
      "is not in pack",
    );
  });
});

describe("summarizePackSelection", () => {
  it("counts the exact selection and names unsafe-tier packages", () => {
    expect(
      summarizePackSelection(pack, ["com.example.base", "com.example.unsafe"]),
    ).toEqual({
      total: 2,
      unsafeIds: ["com.example.unsafe"],
    });
  });

  it("rejects selections outside the reviewed pack revision", () => {
    expect(() => summarizePackSelection(pack, ["com.example.unknown"])).toThrow(
      "is not in pack",
    );
  });
});

describe("packagesForPreset", () => {
  const labelled: Pack = {
    ...pack,
    packages: [
      { ...pack.packages[0], id: "a", labels: ["telemetry"] },
      { ...pack.packages[0], id: "b", labels: ["google", "media"] },
      { ...pack.packages[0], id: "c", labels: ["bloat"] },
      { ...pack.packages[0], id: "d", labels: ["telemetry"] },
    ],
  };
  const ready = new Set(["a", "b", "c"]);

  it("matches ready packages whose labels intersect the preset tags", () => {
    const privacy = DEBLOAT_PRESETS.find((p) => p.id === "privacy")!;
    // `d` also has telemetry but is not ready, so it is excluded.
    expect([...packagesForPreset(labelled, privacy, ready)]).toEqual(["a"]);
  });

  it("matches the de-Google preset by the google label", () => {
    const degoogle = DEBLOAT_PRESETS.find((p) => p.id === "degoogle")!;
    expect([...packagesForPreset(labelled, degoogle, ready)]).toEqual(["b"]);
  });

  it("returns an empty set when nothing matches", () => {
    const carrier = DEBLOAT_PRESETS.find((p) => p.id === "carrier")!;
    expect(packagesForPreset(labelled, carrier, ready).size).toBe(0);
  });
});
