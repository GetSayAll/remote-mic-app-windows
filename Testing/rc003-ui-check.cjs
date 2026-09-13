// Browser-only fixtures. This does not connect to a remote or prove driver support.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const { chromium } = require('playwright');

async function main() {
  const output = path.resolve('artifacts/rc003-filter');
  await fs.mkdir(output, { recursive: true });
  const browser = await chromium.launch({ headless: true, executablePath: process.argv[3] });
  const checks = [];
  try {
    const page = await browser.newPage({ viewport: { width: 1029, height: 732 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(process.argv[2] || 'http://127.0.0.1:2431/', { waitUntil: 'networkidle' });
    assert.deepEqual(await page.locator('.filter-key-status').allTextContents(), Array(3).fill('\u9a71\u52a8\u4fe1\u53f7\u5f85\u786e\u8ba4'));
    for (const card of await page.locator('.mapping-card').all()) {
      if (await card.locator('.filter-key-status').count()) {
        for (const cell of await card.locator('.mapping-cell').all()) assert(await cell.isDisabled());
      }
    }
    await page.screenshot({ path: path.join(output, 'unconfirmed-1029.png'), fullPage: true });
    checks.push('unconfirmed_buttons_disabled');

    // Mutate the existing browser-preview fixture only, then remount the page.
    await page.evaluate(async () => {
      const bridge = await import('/src/lib/bridge.ts');
      const runtime = await bridge.getRuntimeSnapshot();
      runtime.platform.connection.remoteModel = 'rc003';
      runtime.platform.connection.remoteName = 'RC003';
      runtime.platform.rawInput = {
        ...runtime.platform.rawInput, phase: 'ready', matchedDeviceCount: 1,
        confirmedFilterButtons: ['back', 'volume_up', 'volume_down'],
      };
    });
    await page.locator('nav button').nth(1).click();
    await page.locator('nav button').first().click();
    assert.deepEqual(await page.locator('.filter-key-status').allTextContents(), Array(3).fill('\u9a71\u52a8\u4fe1\u53f7\u5df2\u786e\u8ba4'));
    const back = page.locator('.mapping-card').filter({ has: page.locator('.filter-key-status') }).first();
    await back.locator('.mapping-cell').first().click();
    await page.locator('.mapping-editor').waitFor();
    assert((await page.locator('.mapping-editor').innerText()).includes('F13 / F14 / F15'));
    await page.screenshot({ path: path.join(output, 'confirmed-editor-1029.png'), fullPage: true });
    checks.push('confirmed_mapping_editor');
    await page.locator('.mapping-editor .card-title-row').getByRole('button', { name: '\u5173\u95ed', exact: true }).click();

    for (const [width, height, colorScheme] of [[1029, 732, 'dark'], [1440, 900, 'light']]) {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme });
      await page.locator('h1').scrollIntoViewIfNeeded();
      const overlaps = await page.locator('.filter-key-status').evaluateAll(labels => labels.filter(label => {
        const bounds = label.getBoundingClientRect();
        const parent = label.parentElement.getBoundingClientRect();
        const name = label.parentElement.querySelector('strong').getBoundingClientRect();
        return bounds.right > parent.right + 1 || bounds.left < name.right + 2 || label.scrollWidth > label.clientWidth + 1;
      }).length);
      assert.equal(overlaps, 0);
      assert(await page.locator('.remote-photo img').evaluate(image => image.complete && image.naturalWidth > 0));
      await page.screenshot({ path: path.join(output, `confirmed-${width}-${colorScheme}.png`), fullPage: true });
      checks.push(`layout_${width}_${height}_${colorScheme}`);
    }
    assert.deepEqual(errors, []);
    const report = { passed: true, kind: 'browser_fixture_only', checks, errors };
    await fs.writeFile(path.join(output, 'ui-check.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report));
  } finally {
    await browser.close();
  }
}

main().catch(error => { console.error(error); process.exitCode = 1; });
