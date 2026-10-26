import type { Invoke } from "./reader.js";

export interface SecurityStatus {
  configured: boolean;
  locked: boolean;
  epoch: number;
  retry_after_ms: number;
  vault_mode?: "legacy" | "encrypted";
}

export interface SecurityGateOptions {
  root: HTMLElement;
  invoke: Invoke;
  onUnlocked(): void;
}

const SHELL = `
  <main class="security-gate" aria-labelledby="security-title">
    <section class="security-card">
      <div class="brand-lockup security-brand">
        <span class="brand-mark" aria-hidden="true">m</span>
        <span class="brand-name">Mind Place</span>
      </div>
      <p class="eyebrow">Local security gate</p>
      <h1 id="security-title">Unlock Mind Place</h1>
      <p id="security-description" class="security-description"></p>
      <form id="security-form" autocomplete="off">
        <label class="search-label" for="security-secret">PIN or password</label>
        <input id="security-secret" class="security-input" type="password" autocomplete="current-password" minlength="8" maxlength="256" required />
        <div id="security-confirm-row" hidden>
          <label class="search-label security-confirm-label" for="security-secret-confirm">Confirm PIN or password</label>
          <input id="security-secret-confirm" class="security-input" type="password" autocomplete="new-password" minlength="8" maxlength="256" />
        </div>
        <p id="security-error" class="security-error" role="alert" aria-live="assertive"></p>
        <button id="security-submit" class="primary-button security-submit" type="submit">Unlock</button>
      </form>
      <p class="security-note">Mind Place starts locked. Archive access and privileged connectors stay unavailable until this gate is unlocked.</p>
    </section>
  </main>
`;

function errorMessage(value: unknown): string {
  const raw = value instanceof Error ? value.message : String(value ?? "Unlock failed");
  if (raw.includes("MP_SECURITY_RATE_LIMITED")) return "Too many failed attempts. Wait a moment before trying again.";
  if (raw.includes("MP_SECURITY_INVALID_CREDENTIALS")) return "Incorrect PIN or password.";
  if (raw.includes("MP_SECURITY_WEAK_SECRET")) return "Use at least 8 UTF-8 bytes. A longer password is safer than a short PIN.";
  if (raw.includes("MP_SECURITY_ALREADY_CONFIGURED")) return "Security was already configured. Reload and unlock instead.";
  if (raw.includes("MP_SECURITY_INVALID_CONFIG") || raw.includes("MP_SECURITY_UNSUPPORTED_SCHEMA")) {
    return "Security state is invalid or unsupported. Mind Place stayed locked.";
  }
  return "Mind Place could not unlock. It stayed locked.";
}

/**
 * The recovery code is only ever inserted with textContent. Do not persist it,
 * auto-copy it, print it, interpolate it into HTML, or unlock the legacy UI.
 */
async function presentVaultRecovery(root: HTMLElement, invoke: Invoke, code: string): Promise<void> {
  root.innerHTML = `
    <main class="security-gate">
      <section class="security-card" aria-labelledby="vault-title">
        <p class="eyebrow">Encrypted vault setup</p>
        <h1 id="vault-title">Save your offline recovery code</h1>
        <p class="security-description">Store this code somewhere private outside Mind Place. Without it, a Windows account or device change can prevent recovery.</p>
        <pre id="vault-code" class="security-description"></pre>
        <form id="vault-confirm-form" autocomplete="off">
          <label class="search-label" for="vault-confirm-code">Re-enter your recovery code</label>
          <input id="vault-confirm-code" class="security-input" type="password" autocomplete="off" required />
          <label><input id="vault-saved" type="checkbox" required /> I stored this code offline.</label>
          <p id="vault-error" class="security-error" role="alert" aria-live="assertive"></p>
          <button id="vault-confirm" type="submit" class="primary-button security-submit">Confirm encrypted vault</button>
        </form>
      </section>
    </main>
  `;
  const display = root.querySelector<HTMLElement>("#vault-code")!;
  const form = root.querySelector<HTMLFormElement>("#vault-confirm-form")!;
  const entered = root.querySelector<HTMLInputElement>("#vault-confirm-code")!;
  const submit = root.querySelector<HTMLButtonElement>("#vault-confirm")!;
  const error = root.querySelector<HTMLElement>("#vault-error")!;
  display.textContent = code;
  form.addEventListener("submit", async event => {
    event.preventDefault();
    if (submit.disabled) return;
    const confirmation = entered.value;
    entered.value = "";
    if (!root.querySelector<HTMLInputElement>("#vault-saved")!.checked) {
      error.textContent = "Confirm that you saved your recovery code offline.";
      return;
    }
    submit.disabled = true;
    try {
      await invoke("confirm_vault_setup", { code: confirmation });
      // No legacy fallback. The product only becomes available after the
      // encrypted archive and native capture adapters are qualified.
      root.replaceChildren();
      const note = root.ownerDocument.createElement("p");
      note.setAttribute("role", "status");
      note.textContent = "Your encrypted vault was created. Unlock it to access encrypted conversations. Browser capture remains disabled until authenticated native IPC is available.";
      root.append(note);
    } catch {
      error.textContent = "Recovery confirmation failed. Verify the code and try again.";
      submit.disabled = false;
      entered.focus();
    }
  });
  entered.focus();
}

export async function mountSecurityGate({ root, invoke, onUnlocked }: SecurityGateOptions): Promise<void> {
  const status = await invoke<SecurityStatus>("get_security_status");
  if (status.configured && !status.locked) {
    if (status.vault_mode !== "encrypted") {
      onUnlocked();
      return;
    }
    // A verifier being unlocked does not imply the vault runtime is ready.
    // On a remount, show the existing in-memory recovery code again rather
    // than opening the legacy reader or generating a replacement key.
    try {
      const pendingCode = await invoke<string>("resume_vault_setup");
      await presentVaultRecovery(root, invoke, pendingCode);
      return;
    } catch {
      // The owner must re-enter the password to resume provisioning.
    }
  }

  root.innerHTML = SHELL;
  const form = root.querySelector<HTMLFormElement>("#security-form");
  const input = root.querySelector<HTMLInputElement>("#security-secret");
  const confirmRow = root.querySelector<HTMLElement>("#security-confirm-row");
  const confirmInput = root.querySelector<HTMLInputElement>("#security-secret-confirm");
  const submit = root.querySelector<HTMLButtonElement>("#security-submit");
  const description = root.querySelector<HTMLElement>("#security-description");
  const error = root.querySelector<HTMLElement>("#security-error");
  const title = root.querySelector<HTMLElement>("#security-title");
  if (!form || !input || !confirmRow || !confirmInput || !submit || !description || !error || !title) {
    throw new Error("Mind Place security surface is incomplete.");
  }

  // Recovery unlock is owner-local; never send this code to MCP, the
  // browser extension, logs, localStorage, or a URL.
  if (status.configured && status.vault_mode === "encrypted") {
    const panel = root.ownerDocument.createElement("section");
    panel.setAttribute("aria-label", "Offline vault recovery");
    panel.innerHTML = `
      <button id="vault-recover-open" type="button" class="primary-button security-submit">Use offline recovery code</button>
      <form id="vault-recover-form" autocomplete="off" hidden>
        <label class="search-label" for="vault-recover-password">Unlock password</label>
        <input id="vault-recover-password" class="security-input" type="password" autocomplete="off" required />
        <label class="search-label" for="vault-recover-code">Offline recovery code</label>
        <input id="vault-recover-code" class="security-input" type="password" autocomplete="off" required />
        <p id="vault-recover-error" class="security-error" role="alert"></p>
        <button type="submit" class="primary-button security-submit">Recover encrypted vault</button>
      </form>
    `;
    root.querySelector<HTMLElement>(".security-card")!.append(panel);
    const button = panel.querySelector<HTMLButtonElement>("#vault-recover-open")!;
    const recoveryForm = panel.querySelector<HTMLFormElement>("#vault-recover-form")!;
    const password = panel.querySelector<HTMLInputElement>("#vault-recover-password")!;
    const recoveryCode = panel.querySelector<HTMLInputElement>("#vault-recover-code")!;
    const recoveryError = panel.querySelector<HTMLElement>("#vault-recover-error")!;
    button.addEventListener("click", () => {
      recoveryForm.hidden = false;
      button.hidden = true;
      password.focus();
    });
    recoveryForm.addEventListener("submit", async event => {
      event.preventDefault();
      const secret = password.value;
      const code = recoveryCode.value;
      password.value = "";
      recoveryCode.value = "";
      const submitRecovery = recoveryForm.querySelector<HTMLButtonElement>("button[type=submit]")!;
      if (submitRecovery.disabled) return;
      submitRecovery.disabled = true;
      try {
        const next = await invoke<SecurityStatus>("unlock_vault_with_recovery", { secret, code });
        if (next.locked) throw new Error("MP_LOCKED");
        root.replaceChildren();
        onUnlocked();
      } catch {
        recoveryError.textContent = "Recovery failed. Check your password and offline code.";
        submitRecovery.disabled = false;
      }
    });
  }

  const firstRun = !status.configured;
  title.textContent = firstRun ? "Protect Mind Place" : "Unlock Mind Place";
  description.textContent = firstRun
    ? "Create a local unlock secret. This phase locks Mind Place services; it does not yet claim to encrypt existing raw archive files at rest."
    : "Enter your local unlock secret to open the archive and enable privileged services for this runtime epoch.";
  submit.textContent = firstRun ? "Create and unlock" : "Unlock";
  input.autocomplete = firstRun ? "new-password" : "current-password";
  confirmRow.hidden = !firstRun;
  confirmInput.required = firstRun;

  if (status.retry_after_ms > 0) {
    error.textContent = "Unlock is temporarily rate limited after failed attempts.";
  }

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (submit.disabled) return;
    const secret = input.value;
    const confirmation = confirmInput.value;
    input.value = "";
    confirmInput.value = "";
    error.textContent = "";

    if (firstRun && secret !== confirmation) {
      error.textContent = "PIN or password confirmation does not match.";
      input.focus();
      return;
    }

    submit.disabled = true;
    input.disabled = true;
    confirmInput.disabled = true;
    form.setAttribute("aria-busy", "true");
    try {
      const command = firstRun ? "initialize_security" : "unlock_security";
      const next = await invoke<SecurityStatus>(command, { secret });
      if (next.locked) throw new Error("MP_LOCKED");
      root.replaceChildren();
      onUnlocked();
    } catch (cause) {
      const failure = cause instanceof Error ? cause.message : String(cause);
      if (failure.includes("MP_VAULT_PROVISION_REQUIRED") || failure.includes("MP_VAULT_NOT_READY")) {
        try {
          const recoveryCode = await invoke<string>("begin_vault_setup", { secret });
          await presentVaultRecovery(root, invoke, recoveryCode);
          return;
        } catch {
          error.textContent = "Encrypted vault setup is unavailable. No plaintext data was created.";
        }
      } else {
        error.textContent = errorMessage(cause);
      }
      input.disabled = false;
      confirmInput.disabled = false;
      submit.disabled = false;
      form.setAttribute("aria-busy", "false");
      input.focus();
    }
  });

  input.focus();
}
