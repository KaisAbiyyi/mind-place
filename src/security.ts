import type { Invoke } from "./reader.js";

export interface SecurityStatus {
  configured: boolean;
  locked: boolean;
  epoch: number;
  retry_after_ms: number;
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

export async function mountSecurityGate({ root, invoke, onUnlocked }: SecurityGateOptions): Promise<void> {
  const status = await invoke<SecurityStatus>("get_security_status");
  if (status.configured && !status.locked) {
    onUnlocked();
    return;
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
      error.textContent = errorMessage(cause);
      input.disabled = false;
      confirmInput.disabled = false;
      submit.disabled = false;
      form.setAttribute("aria-busy", "false");
      input.focus();
    }
  });

  input.focus();
}
