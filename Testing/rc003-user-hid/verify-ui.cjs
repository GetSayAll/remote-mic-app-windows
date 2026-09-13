const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('playwright');

(async () => {
  const platform = JSON.parse(fs.readFileSync(path.join(__dirname, '../../contracts/ipc/windows-runtime.json'), 'utf8')).platformSnapshot;
  platform.connection.remoteModel = 'rc003';
  platform.connection.remoteName = 'RC003';
  platform.rawInput.activeButtons = [];
  platform.rawInput.confirmedFilterButtons = [];
  const browser = await chromium.launch({ executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(({ platform }) => {
      let callback = 1;
      const state = window.__fixture = { platform, helper: { available: true, phase: 'stopped', reason: null, scope: null, cleanupConfirmed: false }, mappings: { enabled: true, actions: {} } };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
        transformCallback() { return callback++; }, unregisterCallback() {},
        async invoke(command, args) {
          switch (command) {
            case 'get_runtime_snapshot': return { appVersion: '0.2.7', platform: state.platform };
            case 'get_button_mappings': return state.mappings;
            case 'get_button_mapping_snapshot': return state.platform.buttonMapping;
            case 'list_preset_apps': return [];
            case 'get_user_hid_snapshot': return { ...state.helper };
            case 'start_user_hid':
              if (args.acknowledgeRisk !== true) throw new Error('risk_not_acknowledged');
              state.helper.phase = 'ready'; state.helper.scope = 'proxy_unverified'; return { ...state.helper };
            case 'stop_user_hid':
              state.helper.phase = 'stopped'; state.helper.cleanupConfirmed = true;
              state.platform.rawInput.confirmedUserHidButtons = []; return { ...state.helper };
            case 'save_button_mappings': state.mappings = args.mappings; return args.mappings;
            case 'get_theme_preference': return 'light';
            case 'get_app_update_preferences': return { includePrereleases: false };
            case 'check_app_update': return null;
            case 'plugin:event|listen': return callback++;
            default: return null;
          }
        }
      };
    }, { platform });
    await page.goto('http://127.0.0.1:2456', { waitUntil: 'networkidle' });
    const toggle = page.getByRole('checkbox', { name: 'RC003 增强采集' });
    await toggle.click();
    const dialog = page.getByRole('dialog');
    await dialog.waitFor();
    await page.screenshot({ path: path.join(__dirname, '.runs/ui-consent-desktop.png') });
    assert.equal(await page.evaluate(() => document.activeElement.textContent.trim()), '取消');
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(() => document.activeElement.textContent.trim()), '同意并开启');
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(() => document.activeElement.textContent.trim()), '取消');
    await page.getByRole('button', { name: '同意并开启', exact: true }).click();
    await page.evaluate(() => { window.__fixture.platform.rawInput.confirmedUserHidButtons = ['back', 'volume_up', 'volume_down']; });
    await page.getByText('实验信号已确认', { exact: true }).first().waitFor();
    assert.equal(await page.getByText('实验信号已确认', { exact: true }).count(), 3);
    for (const [width, height] of [[1280, 900], [1029, 732]]) {
      await page.setViewportSize({ width, height });
      await page.screenshot({ path: path.join(__dirname, `.runs/ui-ready-${width}.png`), fullPage: true });
      const clipped = await page.locator('.user-hid-bar, .filter-key-status').evaluateAll(nodes => nodes.filter(node => node.scrollWidth > node.clientWidth + 1).length);
      assert.equal(clipped, 0);
    }
    const images = await page.locator('.remote-photo img').evaluateAll(nodes => nodes.map(node => ({ loaded: node.complete && node.naturalWidth > 0 })));
    assert(images.length > 0 && images.every(image => image.loaded));
    await toggle.click();
    await page.waitForFunction(() => document.querySelectorAll('.filter-key-status').length === 3 && [...document.querySelectorAll('.filter-key-status')].every(node => !node.textContent.includes('已确认')));
    await toggle.click();
    await page.setViewportSize({ width: 390, height: 740 });
    await page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; });
    await page.screenshot({ path: path.join(__dirname, '.runs/ui-consent-narrow-dark.png') });
    const box = await dialog.boundingBox();
    assert(box.x >= 0 && box.x + box.width <= 391 && box.y >= 0 && box.y + box.height <= 741);
    assert.equal(await dialog.evaluate(node => node.scrollWidth > node.clientWidth + 1), false);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ result: 'passed', screenshots: 4, desktopWidths: [1280, 1029], narrowDialogWidth: 390, physicalHardware: false }));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
