#!/usr/bin/env node
// Headless boot check for the deployed web build (TODO 34).
//
// The byte-level smoke test (200/content-type/size) cannot catch a wasm that
// dies at init — the binaryen-108 externref table clamp shipped exactly that
// (TADA 49): every static signal passed while the module trapped on
// `WebAssembly.Table.grow(4)` during wasm-bindgen's start glue. The only
// reliable signal is executing the boot path, so this script loads the page
// in headless Chrome (SwiftShader WebGL2) and watches the DOM that
// web/index.html maintains:
//
//   PASS  a <canvas> exists and the #fatal overlay is still hidden
//   FAIL  the #fatal "Something broke" card is shown (init threw or bb-panic
//         fired), or neither outcome arrives before the deadline
//
// Zero dependencies: talks raw Chrome DevTools Protocol over Node >= 22's
// built-in WebSocket. Usage:
//
//   node tools/web_boot_check.mjs <url> [timeout-seconds]   (default 180)
//   CHROME_BIN=/path/to/chrome overrides the browser binary.

import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const url = process.argv[2];
const timeoutSec = Number(process.argv[3] || 180);
if (!url) {
  console.error('usage: web_boot_check.mjs <url> [timeout-seconds]');
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
const profile = mkdtempSync(join(tmpdir(), 'bb-boot-check-'));
const chrome = spawn(chromeBin, [
  '--headless=new',
  '--remote-debugging-port=0',
  '--no-sandbox',
  '--disable-dev-shm-usage',
  // Software WebGL2: CI runners have no GPU, and Chrome >= 128 refuses to
  // fall back to SwiftShader for WebGL without this opt-in.
  '--enable-unsafe-swiftshader',
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

// Chrome prints "DevTools listening on ws://..." to stderr once the
// debugging endpoint is up (port 0 = OS-assigned, so parsing is required).
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
    consoleTail.push(`[console.${msg.params.type}] ${text}`);
    if (consoleTail.length > 20) consoleTail.shift();
  } else if (msg.method === 'Inspector.targetCrashed') {
    finish(1, `FAIL: page crashed (renderer died)\n${consoleTail.join('\n')}`);
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

// The page's own state machine (web/index.html) is the oracle: #status while
// loading, #fatal + #error-detail on failure, a live <canvas> on success.
const PROBE = `JSON.stringify({
  fatalShown: !document.getElementById('fatal')?.classList.contains('hidden'),
  loadingHidden: document.getElementById('loading')?.classList.contains('hidden'),
  canvas: !!document.querySelector('canvas'),
  status: document.getElementById('status')?.textContent || '',
  reason: document.getElementById('fatal-reason')?.textContent || '',
  detail: document.getElementById('error-detail')?.textContent || '',
})`;

// Booted = canvas exists AND the loading overlay is gone (index.html hides it
// only once init() handed control to the browser event loop — the canvas
// alone appears before wgpu finishes initializing). A panic can still land
// moments later, so hold the verdict through a grace window watching #fatal.
const GRACE_MS = 10_000;
const deadline = Date.now() + timeoutSec * 1000;
let lastStatus = '';
let bootedAt = 0;
while (Date.now() < deadline) {
  await new Promise((r) => setTimeout(r, 2000));
  let state;
  try {
    const { result } = await send('Runtime.evaluate', {
      expression: PROBE, returnByValue: true,
    }, sessionId);
    state = JSON.parse(result.value);
  } catch (err) {
    console.log(`probe error (retrying): ${err.message}`);
    continue;
  }
  if (state.fatalShown) {
    finish(1, [
      `FAIL: fatal overlay shown — "${state.reason}"`,
      state.detail && `  detail: ${state.detail}`,
      ...consoleTail,
    ].filter(Boolean).join('\n'));
  }
  if (state.canvas && state.loadingHidden) {
    if (!bootedAt) {
      bootedAt = Date.now();
      console.log('  canvas up, loading overlay gone — holding for late panics');
    } else if (Date.now() - bootedAt >= GRACE_MS) {
      finish(0, 'PASS: canvas up, loading overlay gone, no fatal overlay — the deployed wasm boots');
    }
  }
  if (state.status !== lastStatus) {
    lastStatus = state.status;
    console.log(`  boot progress: ${state.status || '(no status)'}`);
  }
}
finish(1, [
  `FAIL: no canvas and no fatal overlay after ${timeoutSec}s (stuck at "${lastStatus}")`,
  ...consoleTail,
].join('\n'));
