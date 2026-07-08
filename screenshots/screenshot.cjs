// Render the HTML mocks in screenshots/mock/ to PNGs using Playwright.
// Reuses the Playwright install from apps/shelf/screenshots via NODE_PATH
// (CommonJS require respects NODE_PATH; ESM import does not).
// Run: NODE_PATH=<path-to-shelf-screenshots-node_modules> node screenshot.cjs
const { chromium } = require("playwright");
const path = require("path");

const dir = __dirname;
const mock = path.join(dir, "mock");

const shots = [
  { html: "inbox.html", out: "inbox.png", w: 1100, h: 720 },
  { html: "stats.html", out: "stats.png", w: 1100, h: 720 },
];

(async () => {
  const browser = await chromium.launch();
  for (const s of shots) {
    const page = await browser.newPage({ viewport: { width: s.w, height: s.h } });
    await page.goto("file:///" + path.join(mock, s.html).replace(/\\/g, "/"));
    await page.waitForLoadState("networkidle");
    await page.waitForTimeout(900); // tailwind CDN render
    await page.screenshot({ path: path.join(dir, s.out), fullPage: false });
    console.log("shot:", s.out);
    await page.close();
  }
  await browser.close();
})();
