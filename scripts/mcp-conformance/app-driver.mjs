/**
 * Drives the packaged Mind Place app (real WebView2) through the owner-only
 * commands a person would trigger from the Connections screen: unlock, start
 * sharing, create access, choose chats, revoke, Lock. The unlock secret and every
 * issued key stay in this process; callers get them only to place in a client's
 * secure credential slot (an environment variable), never a command line or file.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { chromium } from '@playwright/test';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function portIsBusy(port) {
  return new Promise((resolve) => {
    const probe = http.request({ host: '127.0.0.1', port, method: 'GET', path: '/' }, () => resolve(true));
    probe.on('error', () => resolve(false));
    probe.end();
  });
}

/**
 * @param {{exe: string, runtimeDir: string, dataDir: string}} options
 */
/** An OS-assigned free port: fixed ranges can fall inside Windows' reserved (Hyper-V/WSL) exclusions and never listen. */
async function freePort() {
  return new Promise((resolve, reject) => {
    const probe = http.createServer();
    probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
    probe.on('error', reject);
  });
}

export async function launchApp({ exe, runtimeDir, dataDir }) {
  const cdpPort = await freePort();
  const app = spawn(exe, [], {
    cwd: path.dirname(exe),
    windowsHide: false,
    stdio: 'ignore',
    env: {
      ...process.env,
      MIND_PLACE_DATA_DIR: dataDir,
      WEBVIEW2_USER_DATA_FOLDER: path.join(runtimeDir, 'webview'),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${cdpPort}`,
    },
  });
  let browser;
  const close = async () => {
    await browser?.close().catch(() => {});
    if (app.exitCode === null && app.signalCode === null) app.kill();
    for (let i = 0; i < 25 && app.exitCode === null && app.signalCode === null; i += 1) await sleep(200);
  };
  try {
    const endpoint = `http://127.0.0.1:${cdpPort}`;
    let ok = false;
    for (let i = 0; i < 400 && !ok; i += 1) {
      if (app.exitCode !== null) throw new Error(`App exited early: ${app.exitCode}`);
      try { ok = (await fetch(`${endpoint}/json/version`, { signal: AbortSignal.timeout(600) })).ok; } catch { await sleep(200); }
    }
    assert.ok(ok, 'WebView2 CDP endpoint unavailable');
    browser = await chromium.connectOverCDP(endpoint);
    let page;
    for (let i = 0; i < 80 && !page; i += 1) {
      page = browser.contexts().flatMap((c) => c.pages()).find((p) => p.url() !== 'about:blank');
      if (!page) await sleep(200);
    }
    assert.ok(page, 'App page unavailable');
    page.setDefaultTimeout(20000);
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(String(error.message)));

    /** Owner command. Right after unlock the state migration may still be running; that is a retryable condition. */
    const invoke = async (command, args = {}) => {
      for (let attempt = 0; ; attempt += 1) {
        try {
          return await page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [command, args]);
        } catch (error) {
          if (attempt < 120 && /MP_STATE_BUSY/.test(String(error?.message ?? error))) { await sleep(500); continue; }
          throw error;
        }
      }
    };

    const driver = {
      app,
      page,
      pageErrors,
      invoke,
      /** Provision through the packaged owner commands with synthetic, in-memory credentials. */
      async provisionVault(secret, secrets) {
        const code = await invoke('begin_vault_setup', { secret });
        secrets.push(code);
        await invoke('confirm_vault_setup', { code });
        await invoke('unlock_security', { secret });
        const status = await invoke('get_security_status');
        assert.ok(status.vault_mode === 'encrypted' && status.configured && !status.locked, 'packaged vault is not configured and unlocked');
      },
      async startSharing() {
        const status = await invoke('start_local_mcp');
        assert.equal(status.running, true, 'local MCP listener did not start');
        return status;
      },
      /** Creates a read credential. The caller owns the returned value and must not log it. */
      async issueReader(displayName) {
        const issued = await invoke('issue_local_mcp_reader', { displayName });
        assert.match(issued.credential, /^mpc_v1_[0-9a-f]{64}$/, 'issued key has an unexpected shape');
        return { clientId: issued.client_id, credential: issued.credential };
      },
      async grantableChats(clientId) {
        return (await invoke('list_local_mcp_grantable_chats', { clientId, query: null, offset: 0, limit: 100 })).rows;
      },
      grantSources: (clientId, sourceIds) => invoke('grant_local_mcp_client_sources', { clientId, sourceIds }),
      revokeSources: (clientId, sourceIds) => invoke('revoke_local_mcp_client_sources', { clientId, sourceIds }),
      grantedSources: (clientId) => invoke('list_local_mcp_client_sources', { clientId }),
      revokeClient: (clientId) => invoke('revoke_local_mcp_client', { clientId }),
      lock: () => invoke('lock_security'),
      unlock: (secret) => invoke('unlock_security', { secret }),
      close,
    };
    return driver;
  } catch (error) {
    await close();
    throw error;
  }
}

/** Map conversation title -> source_id, using the owner's grant screen data. */
export async function sourceIdsByTitle(driver, clientId) {
  const rows = await driver.grantableChats(clientId);
  return Object.fromEntries(rows.map((row) => [row.title, row.source_id]));
}

export { fs };
