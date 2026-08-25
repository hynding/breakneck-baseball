#!/usr/bin/env node
// Real-input smoke test for the web build: synthetic *keyboard events* (not
// the Director seam) drive menu → first pitch, proving the actual input
// plugin path — winit key events → gather_intents → flow — works end to end
// in the browser. Everything else goes through the Director by design; this
// is the one test that keeps the real layer honest.
//
// Watches the wasm beacon breadcrumbs (game::autoplay::WebBeaconPlugin,
// always on for the web target):
//
//   "bb-state playing"  after Digit1 starts a one-player game
//   "bb-first-pitch"    after Space releases the first delivery
//
// Zero dependencies (raw CDP over Node >= 22 WebSocket), same harness shape
// as web_boot_check.mjs. Usage:
//
//   node tools/web_input_check.mjs <url> [timeout-seconds]   (default 120)
//   CHROME_BIN=/path/to/chrome overrides the browser binary.

import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const url = process.argv[2];
const timeoutSec = Number(process.argv[3] || 120);
if (!url) {
  console.error('usage: web_input_check.mjs <url> [timeout-seconds]');
  process.exit(2);
}

function findChrome() {
  if (process.env.CHROME_BIN) return process.env.CHROME_BIN;
  const candidates = [
    'google-chrome',
    'chromium-browser',
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  ];
  for (const c of candidates) {
    try {
      execFileSync(c, ['--version'], { stdio: 'ignore' });
      return c;
    } catch {}
  }
  console.error('no Chrome binary found (set CHROME_BIN)');
  process.exit(2);
}

const chromeBin = findChrome();
const profile = mkdtempSync(join(tmpdir(), 'bb-input-check-'));
const chrome = spawn(chromeBin, [
  '--headless=new',
  '--remote-debugging-port=0',
  '--no-sandbox',
  '--disable-dev-shm-usage',
  '--enable-unsafe-swiftshader',
  // The page needs no user gesture for audio in this check, but mute anyway.
  '--mute-audio',
  `--user-data-dir=${profile}`,
  'about:blank',
]);

let finished = false;
function finish(code, message) {
  if (finished) return;
  finished = true;
  console.log(message);
  chrome.kill('SIGKILL');
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
  process.exit(code);
}

chrome.on('exit', (code) => {
  if (!finished) finish(2, `FAIL: chrome exited early (code ${code})`);
});

const wsUrl = await new Promise((resolve, reject) => {
  let buf = '';
  const timer = setTimeout(() => reject(new Error('no DevTools endpoint after 30s')), 30_000);
  chrome.stderr.on('data', (chunk) => {
    buf += chunk;
    const m = buf.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) { clearTimeout(timer); resolve(m[1]); }
  });
}).catch((err) => finish(2, `FAIL: ${err.message}`));

const ws = new WebSocket(wsUrl);
let nextId = 1;
const pending = new Map();

function send(method, params = {}, sessionId) {
  const id = nextId++;
  ws.send(JSON.stringify({ id, method, params, ...(sessionId && { sessionId }) }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}

// Beacon breadcrumbs observed so far, plus a console tail for diagnostics.
const seen = new Set();
const consoleTail = [];
ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
  } else if (msg.method === 'Runtime.consoleAPICalled') {
    const text = (msg.params.args || [])
      .map((a) => a.value ?? a.description ?? '')
      .join(' ');
    if (text.startsWith('bb-')) seen.add(text.trim());
    consoleTail.push(`[console.${msg.params.type}] ${text}`);
    if (consoleTail.length > 20) consoleTail.shift();
  } else if (msg.method === 'Inspector.targetCrashed') {
    finish(1, `FAIL: page crashed\n${consoleTail.join('\n')}`);
  }
});
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve);
  ws.addEventListener('error', () => reject(new Error('WebSocket connect failed')));
}).catch((err) => finish(2, `FAIL: ${err.message}`));

const { targetId } = await send('Target.createTarget', { url });
const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
await send('Runtime.enable', {}, sessionId);
await send('Inspector.enable', {}, sessionId);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function booted() {
  const { result } = await send('Runtime.evaluate', {
    expression: `!!document.querySelector('canvas') &&
                 document.getElementById('loading')?.classList.contains('hidden')`,
    returnByValue: true,
  }, sessionId);
  return !!result.value;
}

// Winit reads real key events off the canvas/window: dispatch a full
// down+up pair with code+key so the browser produces a genuine KeyboardEvent.
async function tapKey(key, code, keyCode) {
  for (const type of ['keyDown', 'keyUp']) {
    await send('Input.dispatchKeyEvent', {
      type, key, code,
      windowsVirtualKeyCode: keyCode,
      nativeVirtualKeyCode: keyCode,
    }, sessionId);
    await sleep(80);
  }
}

async function clickCanvas() {
  // Focus + the user gesture the audio context wants.
  const { result } = await send('Runtime.evaluate', {
    expression: `(() => { const c = document.querySelector('canvas');
      if (!c) return null; const r = c.getBoundingClientRect();
      return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
    returnByValue: true,
  }, sessionId);
  const at = result.value;
  if (!at) return;
  for (const type of ['mousePressed', 'mouseReleased']) {
    await send('Input.dispatchMouseEvent', {
      type, x: at.x, y: at.y, button: 'left', clickCount: 1,
    }, sessionId);
    await sleep(60);
  }
}

const deadline = Date.now() + timeoutSec * 1000;
async function waitFor(crumb, label) {
  while (Date.now() < deadline) {
    if (seen.has(crumb)) { console.log(`  ${label}`); return; }
    await sleep(500);
  }
  finish(1, [
    `FAIL: never saw "${crumb}" (${label}) within ${timeoutSec}s`,
    `  crumbs seen: ${[...seen].join(', ') || '(none)'}`,
    ...consoleTail,
  ].join('\n'));
}

// 1. Boot.
while (Date.now() < deadline && !(await booted().catch(() => false))) {
  await sleep(1000);
}
if (!(await booted().catch(() => false))) {
  finish(1, `FAIL: page never booted within ${timeoutSec}s\n${consoleTail.join('\n')}`);
}
console.log('  booted');

// 2. Real keyboard: 1 starts a one-player game (menu path).
await clickCanvas();
await tapKey('1', 'Digit1', 0x31);
await waitFor('bb-state playing', 'Digit1 started a game through the real menu');

// 3. Real keyboard: Space releases the first pitch (Home fields the top).
// A few taps with gaps — the first press may land during scene spawn.
for (let i = 0; i < 5 && !seen.has('bb-first-pitch'); i++) {
  await tapKey(' ', 'Space', 0x20);
  await sleep(700);
}
await waitFor('bb-first-pitch', 'Space released the first pitch');

finish(0, 'PASS: real keyboard events drove menu -> first pitch through the actual input plugin');
