/**
 * Capture README screenshots from a running cortex-server.
 * Usage: node scripts/capture-screens.mjs
 */
import { createRequire } from "module";
const require = createRequire(import.meta.url);
const playwrightRoot =
  process.env.PLAYWRIGHT_CORE ||
  "C:/Users/SergiiZiborov/Documents/GitHub/MyProjects/loom-studio/node_modules/playwright-core";
const { chromium } = require(playwrightRoot);
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(__dirname, "..", "docs", "images");
fs.mkdirSync(outDir, { recursive: true });

const edge =
  process.env.EDGE_PATH ||
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const chrome =
  process.env.CHROME_PATH ||
  "C:\\Users\\SergiiZiborov\\AppData\\Local\\ms-playwright\\chromium-1228\\chrome-win64\\chrome.exe";
const executablePath = fs.existsSync(edge) ? edge : chrome;

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--disable-gpu", "--no-first-run"],
});
const page = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  deviceScaleFactor: 2,
});
page.setDefaultTimeout(30000);

async function shot(name) {
  const dest = path.join(outDir, name);
  await page.screenshot({ path: dest, fullPage: false });
  console.log("wrote", name, fs.statSync(dest).size);
}

await page.goto("http://127.0.0.1:43817/", { waitUntil: "networkidle" });
await page.waitForSelector(".graph-canvas, .app-shell, svg", { timeout: 25000 });
await page.waitForTimeout(1500);
await shot("studio-canvas.png");

const sequences = page.locator(".toolbar-actions-desktop button.sequence-button");
if ((await sequences.count()) > 0 && (await sequences.first().isVisible())) {
  await sequences.first().click();
} else {
  await page.locator(".toolbar-actions-menu summary").first().click();
  await page.locator(".toolbar-actions-menu button").filter({ hasText: /^Sequences$/ }).click();
}
await page.waitForSelector(".sequence-studio, [aria-label='Sequence studio']");
await page.waitForTimeout(800);
await shot("studio-sequences.png");
await page.keyboard.press("Escape");
await page.waitForTimeout(400);

const help = page.locator(".header-actions-desktop button").filter({ hasText: /^Help$/ });
if ((await help.count()) > 0 && (await help.first().isVisible())) {
  await help.first().click();
} else {
  await page.locator(".app-header details summary, .header-menu summary").first().click({ timeout: 3000 }).catch(() => {});
  await page.locator("button").filter({ hasText: /^Help$/ }).last().click({ force: true });
}
await page.waitForTimeout(800);
await shot("studio-docs.png");
await page.keyboard.press("Escape");

await browser.close();
