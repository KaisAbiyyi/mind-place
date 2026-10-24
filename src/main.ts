import { invoke } from "@tauri-apps/api/core";
import "katex/dist/katex.min.css";
import "./style.css";
import "./knowledge-os.css";
import "./memory-inspector.css";
import "./security.css";
import type { Invoke } from "./reader.js";
import { mountApplication } from "./app.js";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Mind Place reader root was not found.");

const tauriInvoke: Invoke = <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args);

mountApplication({ root, invoke: tauriInvoke }).catch(() => {
  root.innerHTML = `
    <main class="security-gate" aria-labelledby="security-failure-title">
      <section class="security-card">
        <p class="eyebrow">Fail-closed startup</p>
        <h1 id="security-failure-title">Mind Place stayed locked</h1>
        <p class="security-description">The local security gate could not be loaded. Restart Mind Place or inspect local diagnostics before accessing the archive.</p>
      </section>
    </main>
  `;
});
