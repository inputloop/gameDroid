// SPDX-License-Identifier: GPL-3.0-or-later
// Renders captured terminal text as a PNG for the README.
// The text is copied verbatim from CI job logs; this only draws it.
// Usage: node tools/screenshots/render-terminal.js <input.txt> <output.png> "<title>"
const fs = require('fs');
const { chromium } = require('playwright');
(async () => {
  const [input, output, title] = process.argv.slice(2);
  const text = fs.readFileSync(input, 'utf8').replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const html = `<html><body style="margin:0;background:#0d1117">
  <div id="t" style="display:inline-block;min-width:860px;font:14px/1.5 'DejaVu Sans Mono',monospace;color:#c9d1d9;background:#0d1117;padding:0 0 14px 0">
  <div style="background:#161b22;color:#8b949e;padding:8px 16px;font-size:13px;border-bottom:1px solid #30363d">${title}</div>
  <pre style="margin:14px 16px 0 16px;font:inherit;white-space:pre">${text}</pre></div></body></html>`;
  const browser = await chromium.launch();
  const page = await browser.newPage({ deviceScaleFactor: 2 });
  await page.setContent(html);
  await (await page.$('#t')).screenshot({ path: output });
  await browser.close();
})();
