#!/usr/bin/env node
// Frame cost per graphics tier, for the web build.
//
// Loads the page once per tier with `?perf&quality=<tier>` (the tier is
// forced for that run only; the saved setting is untouched), starts a
// one-player game (the CPU pitches, the human side idles), and collects
// the game's `bb-perf` console lines: mean and worst frame, entity count,
// and meshes drawn, every 5 s. Prints a Markdown table of the medians per
// tier and writes report.json plus one screenshot per tier.
//
// Every presentation PR in the graphics and camera plan runs this before
// and after, so its cost per tier is a number rather than a guess. Under
// software WebGL (a cloud container) the absolute frame times say nothing
// about real devices; compare runs from the same machine only. Natively,
// `BREAKNECK_PERF=1 BREAKNECK_QUALITY=low cargo run --release` prints the
// same lines.
//
//   node tools/frame_cost.mjs <url> [out-dir] [seconds-per-tier] [viewport WxH] [menu-keys]
//
// `menu-keys` are main-menu hotkey letters pressed before the game starts,
// e.g. `wwww` for Rain or `wwwwn` for Rain at Night (W cycles the weather
// Fair, Hot, Cold, Windy, Rain; N cycles the time of day from Afternoon to
// Night, then Morning).

import { createRequire } from 'node:module';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const { chromium } = createRequire(import.meta.url)('playwright');

const base = process.argv[2];
const outDir = process.argv[3] || 'frame-cost';
const secs = Number(process.argv[4] || 60);
const [vw, vh] = (process.argv[5] || '1280x720').split('x').map(Number);
const menuKeys = process.argv[6] || '';
if (!base) {
  console.error('usage: frame_cost.mjs <url> [out-dir] [seconds-per-tier] [WxH] [menu-keys]');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const median = (xs) => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const parse = (line) => {
  const m = line.match(/bb-perf tier=(\w+) frames=(\d+) mean_ms=([\d.]+) worst_ms=([\d.]+) entities=(\d+) meshes_visible=(\d+)/);
  return m && { tier: m[1], frames: +m[2], mean: +m[3], worst: +m[4], entities: +m[5], visible: +m[6] };
};

const browser = await chromium.launch({
  args: ['--enable-unsafe-swiftshader', '--use-angle=swiftshader', '--ignore-gpu-blocklist'],
});
const report = {};
for (const tier of (process.env.TIERS||'low,medium,high').split(',')) {
  const page = await browser.newPage({ viewport: { width: vw, height: vh }, deviceScaleFactor: 1 });
  const logs = [];
  page.on('console', (m) => logs.push(m.text()));
  page.on('pageerror', (e) => logs.push(`PAGEERROR ${e.message}`));
  const url = new URL(base);
  url.searchParams.set('quality', tier);
  url.search = url.search + '&perf';
  await page.goto(url.toString());
  const until = async (re, ms) => {
    const t0 = Date.now();
    while (!logs.some((l) => re.test(l)) && Date.now() - t0 < ms) await page.waitForTimeout(500);
  };
  await until(/bb-state menu/, 240_000);
  await page.waitForTimeout(2000);
  await page.mouse.click(10, 10);
  for (const k of menuKeys) {
    await page.keyboard.press(`Key${k.toUpperCase()}`);
    await page.waitForTimeout(300);
  }
  await page.keyboard.press('Digit1');
  await until(/bb-state playing/, 120_000);
  const startAt = logs.length;
  await page.waitForTimeout(secs * 1000);
  await page.screenshot({ path: join(outDir, `${tier}.png`) });
  // The first window straddles boot and the menu; drop it.
  const label = tier[0].toUpperCase() + tier.slice(1);
  const rows = logs
    .slice(startAt)
    .map(parse)
    .filter((r) => r && r.tier === label)
    .slice(1);
  if (!rows.length) console.error(`${tier}: no bb-perf windows (run at least ~12 s per tier, and check the tier was forced)`);
  report[tier] = {
    forced: logs.some((l) => /tier forced to/.test(l)),
    windows: rows.length,
    meanMs: median(rows.map((r) => r.mean)),
    worstMs: median(rows.map((r) => r.worst)),
    entities: median(rows.map((r) => r.entities)),
    meshesVisible: median(rows.map((r) => r.visible)),
    panics: logs.filter((l) => /panicked|PAGEERROR|bb-panic/.test(l)),
  };
  writeFileSync(join(outDir, `${tier}-console.log`), logs.join('\n'));
  await page.close();
}
await browser.close();
writeFileSync(join(outDir, 'report.json'), JSON.stringify(report, null, 2));

const fmt = (v, d = 1) => (v == null ? '–' : v.toFixed(d));
console.log(`| Tier | Mean frame (ms) | Worst frame (ms) | Entities | Meshes visible |`);
console.log(`| --- | --- | --- | --- | --- |`);
for (const [tier, r] of Object.entries(report)) {
  console.log(`| ${tier} | ${fmt(r.meanMs)} | ${fmt(r.worstMs)} | ${fmt(r.entities, 0)} | ${fmt(r.meshesVisible, 0)} |`);
}
const failed = Object.values(report).some((r) => r.panics.length || !r.forced || !r.windows);
process.exit(failed ? 1 : 0);
