#!/usr/bin/env node
/* global document */

import fs from "node:fs";
import path from "node:path";
import { stdout } from "node:process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const outputPath = path.join(repoRoot, ".github", "social-preview.png");
const logoPath = path.join(repoRoot, "src", "assets", "droidsmith-logo.png");
const overviewPath = path.join(
  repoRoot,
  "docs",
  "screenshots",
  "droidsmith-overview.png",
);
const appsPath = path.join(
  repoRoot,
  "docs",
  "screenshots",
  "droidsmith-apps.png",
);

for (const requiredPath of [logoPath, overviewPath, appsPath]) {
  if (!fs.existsSync(requiredPath)) {
    throw new Error(
      `Missing marketing source: ${path.relative(repoRoot, requiredPath)}`,
    );
  }
}

const pkg = JSON.parse(
  fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"),
);

function pngDataUri(filePath) {
  return `data:image/png;base64,${fs.readFileSync(filePath).toString("base64")}`;
}

const logo = pngDataUri(logoPath);
const overview = pngDataUri(overviewPath);
const apps = pngDataUri(appsPath);
const browser = await chromium.launch();

try {
  const page = await browser.newPage({
    viewport: { width: 1280, height: 640 },
    deviceScaleFactor: 1,
  });
  await page.setContent(
    `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <style>
      * { box-sizing: border-box; }
      html, body { width: 1280px; height: 640px; margin: 0; overflow: hidden; }
      body {
        position: relative;
        color: #f7fbff;
        background:
          radial-gradient(circle at 78% 16%, rgba(10, 200, 235, 0.18), transparent 34%),
          radial-gradient(circle at 10% 92%, rgba(67, 97, 238, 0.16), transparent 40%),
          #090b10;
        font-family: "Segoe UI Variable Display", "Segoe UI", Arial, sans-serif;
      }
      body::before {
        content: "";
        position: absolute;
        inset: 0;
        opacity: 0.18;
        background-image:
          linear-gradient(rgba(255,255,255,0.045) 1px, transparent 1px),
          linear-gradient(90deg, rgba(255,255,255,0.045) 1px, transparent 1px);
        background-size: 42px 42px;
        mask-image: linear-gradient(90deg, black, transparent 55%);
      }
      .copy {
        position: absolute;
        z-index: 3;
        left: 64px;
        top: 62px;
        width: 455px;
      }
      .brand { display: flex; align-items: center; gap: 19px; }
      .logo {
        width: 94px;
        height: 94px;
        border-radius: 24px;
        box-shadow: 0 22px 54px rgba(0, 196, 235, 0.18);
      }
      .name {
        margin: 0;
        font-size: 61px;
        line-height: 0.95;
        letter-spacing: -2.9px;
        font-weight: 720;
      }
      .version {
        display: inline-block;
        margin-top: 12px;
        color: #7eeaff;
        font-size: 16px;
        font-weight: 650;
        letter-spacing: 0.7px;
      }
      h2 {
        margin: 52px 0 18px;
        max-width: 420px;
        font-size: 43px;
        line-height: 1.07;
        letter-spacing: -1.5px;
        font-weight: 680;
      }
      p {
        margin: 0;
        max-width: 420px;
        color: #aeb8c7;
        font-size: 22px;
        line-height: 1.42;
      }
      .local {
        display: flex;
        align-items: center;
        gap: 9px;
        margin-top: 34px;
        color: #d9faff;
        font-size: 17px;
        font-weight: 600;
      }
      .local::before {
        content: "";
        width: 9px;
        height: 9px;
        border-radius: 50%;
        background: #20d6a0;
        box-shadow: 0 0 0 6px rgba(32,214,160,0.12);
      }
      .stage {
        position: absolute;
        z-index: 2;
        left: 526px;
        top: 55px;
        width: 800px;
        height: 560px;
      }
      .window {
        position: absolute;
        overflow: hidden;
        background: #11151d;
        border: 1px solid rgba(165, 232, 245, 0.18);
        border-radius: 18px;
        box-shadow: 0 34px 82px rgba(0,0,0,0.56);
      }
      .window::before {
        content: "";
        display: block;
        height: 24px;
        border-bottom: 1px solid rgba(255,255,255,0.06);
        background:
          radial-gradient(circle at 16px 12px, #ff6262 0 4px, transparent 4.5px),
          radial-gradient(circle at 32px 12px, #ffc153 0 4px, transparent 4.5px),
          radial-gradient(circle at 48px 12px, #3bd58c 0 4px, transparent 4.5px),
          #151922;
      }
      .window img { display: block; width: 100%; height: calc(100% - 24px); object-fit: cover; object-position: top left; }
      .overview { left: 68px; top: 0; width: 695px; height: 458px; transform: rotate(1.2deg); }
      .apps { left: 0; top: 316px; width: 416px; height: 274px; transform: rotate(-2.4deg); }
      .edge {
        position: absolute;
        right: -10px;
        top: 0;
        bottom: 0;
        width: 120px;
        background: linear-gradient(90deg, transparent, #090b10 88%);
      }
    </style>
  </head>
  <body>
    <main class="copy">
      <div class="brand">
        <img class="logo" src="${logo}" alt="">
        <div>
          <h1 class="name">Droidsmith</h1>
          <span class="version">VERSION ${pkg.version}</span>
        </div>
      </div>
      <h2>Your Android workshop.</h2>
      <p>Inspect apps, recover debloat changes, pair over Wi-Fi, read Logcat, and launch scrcpy from one desktop.</p>
      <div class="local">Local tools. No account or telemetry.</div>
    </main>
    <section class="stage" aria-hidden="true">
      <div class="window overview"><img src="${overview}" alt=""></div>
      <div class="window apps"><img src="${apps}" alt=""></div>
    </section>
    <div class="edge"></div>
  </body>
</html>`,
    { waitUntil: "load" },
  );
  await page.evaluate(() => document.fonts.ready);
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });
  await page.screenshot({ path: outputPath });
  stdout.write(
    `Wrote ${path.relative(repoRoot, outputPath)} for Droidsmith v${pkg.version}\n`,
  );
} finally {
  await browser.close();
}
