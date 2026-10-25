const { chromium } = require("playwright-core");

(async () => {
  const browser = await chromium.launch({
    executablePath:
      "C:\\Users\\Gort\\AppData\\Local\\ms-playwright\\chromium-1248\\chrome-win64\\chrome.exe",
    headless: true,
  });
  const ctx = await browser.newContext({
    viewport: { width: 900, height: 880 },
    deviceScaleFactor: 2,
    colorScheme: "dark",
  });
  const page = await ctx.newPage();
  await page.goto("http://127.0.0.1:5173/#/rules", { waitUntil: "networkidle" });
  await page.waitForTimeout(1400);
  const box = await page.getByRole("button", { name: "删除订阅" }).first().boundingBox();
  console.log(JSON.stringify(box));
  await page.screenshot({ path: "docs/screenshots/rules.png" });
  await browser.close();
})();
