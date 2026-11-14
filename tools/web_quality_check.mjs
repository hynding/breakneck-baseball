#!/usr/bin/env node
// Headless graphics-quality check for the web build.
//
// Loads the page in Chromium (Playwright, software WebGL2) twice: once as a
// desktop browser with a mouse and once emulating a phone (touch, coarse
// pointer, 3x screen). For each it records the tier Auto picked (the
// `graphics: platform ...` log line), any adaptive step-down
// (`Auto steps down to ...`), the requestAnimationFrame rate, panics, and a
// screenshot. A software rasterizer is far slower than any real GPU, so the
// frame rates are not device numbers; what this proves is that each path
// boots, picks the tier it should, renders, and falls back when frames are
// slow, in a live one-player game. Real phones and GPUs still need a person (docs/DEVICE-CHECKS.md).
//
//   node tools/web_quality_check.mjs <url> [out-dir] [seconds-per-profile]

import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// require() rather than import: it honours NODE_PATH, where a global
// Playwright install lives.
const { chromium, devices } = createRequire(import.meta.url)('playwright');

const url = process.argv[2];
const outDir = process.argv[3] || 'quality-check';
const secs = Number(process.argv[4] || 45);
if (!url) {
  console.error('usage: web_quality_check.mjs <url> [out-dir] [seconds]');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const profiles = [
  { name: 'desktop', context: { viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 } },
  { name: 'phone', context: { ...devices['iPhone 13'] } },
];

const browser = await chromium.launch({
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader', '--ignore-gpu-blocklist'],
});
const report = {};
for (const p of profiles) {
  const context = await browser.newContext(p.context);
  const page = await context.newPage();
  const logs = [];
  page.on('console', (m) => logs.push(m.text()));
  page.on('pageerror', (e) => logs.push(`PAGEERROR ${e.message}`));
  await page.goto(url);
  await page.waitForSelector('canvas', { timeout: 180_000 });
  const coarse = await page.evaluate(() => matchMedia('(pointer: coarse)').matches);
  // Start a one-player game so the park, players and shadows render (the
  // menu alone is mostly UI). The human side idles; the CPU keeps pitching.
  await page.waitForTimeout(8000);
  // Phones first get the touch-controls note; its Play button dismisses it.
  const note = page.locator('#touch-note button');
  if (await note.isVisible().catch(() => false)) await note.click();
  await page.mouse.click(10, 10);
  await page.keyboard.press('Digit1');
  // Let it boot and run, sampling the frame rate in 5-second slices.
  const fps = [];
  const deadline = Date.now() + secs * 1000;
  while (Date.now() < deadline) {
    fps.push(
      await page.evaluate(
        () =>
          new Promise((resolve) => {
            let n = 0;
            const t0 = performance.now();
            const tick = () => {
              n++;
              if (performance.now() - t0 < 5000) requestAnimationFrame(tick);
              else resolve((n * 1000) / (performance.now() - t0));
            };
            requestAnimationFrame(tick);
          }),
      ),
    );
  }
  await page.screenshot({ path: join(outDir, `${p.name}.png`) });
  const pick = (re) => logs.filter((l) => re.test(l)).map((l) => l.replace(/\s+/g, ' ').trim());
  report[p.name] = {
    coarsePointer: coarse,
    devicePixelRatio: await page.evaluate(() => devicePixelRatio),
    autoPick: pick(/graphics: platform/),
    stepDowns: pick(/Auto steps down/),
    panics: pick(/panicked|PAGEERROR|bb-panic/),
    fpsSlices: fps.map((f) => Math.round(f * 10) / 10),
  };
  writeFileSync(join(outDir, `${p.name}-console.log`), logs.join('\n'));
  await context.close();
}
await browser.close();
writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
const failed = Object.values(report).some((r) => r.panics.length || !r.autoPick.length);
process.exit(failed ? 1 : 0);
