// Browser fixtures only. No native connection, input injection, or device changes.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require('playwright');

async function main() {
  const output = path.resolve('artifacts/remote-battery');
  await fs.mkdir(output, { recursive: true });
  const browser = await chromium.launch({ headless: true, executablePath: process.argv[3] });
  const checks = [];
  const errors = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1029, height: 732 } });
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/src/lib/bridge.ts*', async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `${await response.text()}\nwindow.__setBatteryFixture = (phase, level) => {
        browserSnapshot.platform.connection = { ...browserSnapshot.platform.connection,
          phase, batteryLevel: level, remoteModel: 'rc003', remoteName: 'Xiaomi Bluetooth Remote 2 Pro' };
      };` });
    });
    await page.goto(process.argv[2] || 'http://127.0.0.1:2431/', { waitUntil: 'networkidle' });
    assert.equal((await page.locator('.battery-indicator').innerText()).trim(), '\u7535\u91cf\u672a\u77e5');
    checks.push('unknown_without_connection');

    async function fixture(phase, level, tabIndex = 0) {
      await page.evaluate(({ phase, level }) => window.__setBatteryFixture(phase, level), { phase, level });
      await page.locator('nav button').nth(3).click();
      await page.locator('nav button').nth(tabIndex).click();
      await page.locator('.battery-indicator').waitFor();
      const expected = ['ready', 'streaming'].includes(phase) ? `${level}%` : '\u7535\u91cf\u672a\u77e5';
      await page.waitForFunction(expected => document.querySelector('.battery-indicator')?.textContent?.trim() === expected, expected);
    }

    for (const [width, height, colorScheme] of [[980, 720, 'light'], [1029, 732, 'dark'], [1440, 900, 'light']]) {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme });
      await fixture('ready', 99);
      assert.equal((await page.locator('.battery-indicator').innerText()).trim(), '99%');
      assert((await page.locator('.battery-indicator').getAttribute('title')).includes('Windows'));
      const layout = await page.locator('.device-chip').evaluate(chip => {
        const battery = chip.querySelector('.battery-indicator').getBoundingClientRect();
        const name = chip.children[1].getBoundingClientRect();
        const box = chip.getBoundingClientRect();
        const header = chip.closest('header').getBoundingClientRect();
        return battery.left > name.right && battery.right <= box.right && box.right <= header.right + 1;
      });
      assert(layout);
      await page.screenshot({ path: path.join(output, `battery-${width}-${colorScheme}.png`), fullPage: true });
      checks.push(`mapping_${width}_${height}_${colorScheme}`);
    }
    await fixture('streaming', 0);
    assert.equal((await page.locator('.battery-indicator').innerText()).trim(), '0%');
    assert((await page.locator('.battery-indicator').getAttribute('class')).includes('low'));
    checks.push('zero_and_low_battery');
    for (const phase of ['disconnected', 'reconnecting', 'suspended']) {
      await fixture(phase, 99);
      assert.equal((await page.locator('.battery-indicator').innerText()).trim(), '\u7535\u91cf\u672a\u77e5');
      checks.push(`stale_hidden_${phase}`);
    }
    await fixture('ready', 99, 1);
    assert.equal((await page.locator('.battery-indicator').innerText()).trim(), '99%');
    await page.screenshot({ path: path.join(output, 'connection-battery.png'), fullPage: true });
    checks.push('connection_page_battery');
    assert.deepEqual(errors, []);
    const report = { passed: true, kind: 'browser_fixture_only', checks, errors };
    await fs.writeFile(path.join(output, 'ui-check.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
