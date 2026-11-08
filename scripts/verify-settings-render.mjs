// Visual smoke of the real, merged Settings shell at multiple viewports (Edge on Windows).
// This uses a synthetic native invoke surface, never an owner's archive or browser profile.
import assert from 'node:assert/strict';
import { chromium, expect } from '@playwright/test';
import { createServer } from 'vite';

const viewports = [[1180, 820], [1180, 600], [800, 520], [590, 410], [390, 700], [360, 520]];
const vite = await createServer({ server: { host: '127.0.0.1', port: 14476, strictPort: true }, logLevel: 'error' });
let browser;
try {
  await vite.listen();
  // Use the installed Edge, without opening or taking over the user's browser profile.
  browser = await chromium.launch({ headless: true, timeout: 15000, executablePath: process.platform === 'win32'
    ? 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe'
    : process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const results = [];
  for (const [width, height] of viewports) {
    console.log(`Checking Settings at ${width}x${height}`);
    await page.setViewportSize({ width, height });
    await page.goto('http://127.0.0.1:14476/');
    await page.waitForTimeout(100);
    await page.evaluate(async () => {
      const { mountReader } = await import('/src/reader.ts');
      const root = document.querySelector('#app');
      const invoke = async (command) => {
        if (command === 'list_conversations' || command === 'list_document_roots' || command === 'list_local_mcp_clients') return [];
        if (command === 'get_local_mcp_status') return { running: false, endpoint: '' };
        if (command === 'get_hosted_tunnel_status') return { running: false, tunnel_id: null, connection_verified: false };
        if (command === 'get_setup') return {
          archive_path: 'D:\\MindPlace\\archive', extension_path: 'D:\\extension', host_path: 'D:\\native-host.exe',
          extension_id: 'synthetic-extension', native_host_name: 'host', platform: 'Windows',
          registered_browsers: [], legacy_available: false,
        };
        throw new Error('unexpected native command ' + command);
      };
      const handle = mountReader({ root, invoke });
      await handle.ready;
    });
    // Exercise the public navigation path: the sidebar opens the wizard, not the dialog.
    await page.locator('#knowledge-os-nav [data-route="setup-onboarding"]').click();
    await expect(page.locator('.onboarding-workspace')).toBeVisible();
    await expect(page.locator('#setup-dialog')).not.toBeVisible();
    await page.locator('.onboarding-workspace button[data-step="providers"]').click();
    await page.getByRole('button', { name: 'Open full capture settings', exact: true }).click();
    await page.waitForSelector('#settings-panel-archive [data-action="import-generic"]');
    const result = await page.evaluate(() => {
      const dialog = document.querySelector('#setup-dialog');
      const content = document.querySelector('#setup-content');
      const panel = document.querySelector('#settings-panel-archive');
      for (let i = 0; i < 25; i++) {
        const p = document.createElement('p');
        p.textContent = `Long overflow qualification paragraph ${i}: ${'Document controls must remain reachable. '.repeat(9)}`;
        panel.append(p);
      }
      const footer = dialog.querySelector('.dialog-footer');
      const header = dialog.querySelector('.dialog-header');
      content.scrollTop = content.scrollHeight;
      const cr = content.getBoundingClientRect();
      const fr = footer.getBoundingClientRect();
      const hr = header.getBoundingClientRect();
      return {
        dialogOpen: dialog.hasAttribute('open'),
        headerVisible: hr.top >= -1 && hr.bottom <= innerHeight + 1,
        footerVisible: fr.top >= -1 && fr.bottom <= innerHeight + 1,
        scrollable: content.scrollHeight > content.clientHeight && content.scrollTop > 0,
        noHorizontalOverflow: dialog.scrollWidth <= dialog.clientWidth + 1,
        scrollAreaAboveFooter: cr.bottom <= fr.top + 1,
        archiveTabSelected: document.querySelector('#settings-tab-archive')?.getAttribute('aria-selected') === 'true',
        genericImportReachable: !!document.querySelector('#settings-panel-archive [data-action="import-generic"]'),
      };
    });
    // A DOM node alone is not reachability: normal Playwright clicks must scroll to
    // the real import button and execute its safe no-file validation, then close via Done.
    await page.locator('#settings-panel-archive [data-action="import-generic"]').click();
    await expect(page.locator('[data-generic-status]')).toHaveText('Choose a JSON, Markdown or HTML file first.');
    await page.locator('#setup-dialog .dialog-footer [data-action="close-setup"]').click();
    await expect(page.locator('#setup-dialog')).not.toBeVisible();
    await expect(page.locator('.onboarding-workspace')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open full capture settings', exact: true })).toBeFocused();
    results.push({ viewport: `${width}x${height}`, ...result });
    assert.ok(Object.values(result).every(Boolean), `Settings visual regression at ${width}x${height}: ${JSON.stringify(result)}`);
  }
  assert.deepEqual(errors, [], `Uncaught browser errors: ${errors.join('; ')}`);
  console.log(JSON.stringify({ passed: results.length, results }, null, 2));
} finally {
  await browser?.close();
  await vite.close();
}
