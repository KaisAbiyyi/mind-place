import type { Invoke } from "./reader.js";

interface SecuritySnapshot { configured: boolean; locked: boolean; epoch: number }
interface IndexSnapshot { sources: number; revisions: number; chunks: number; generation: number }
interface LocalSnapshot { running: boolean }
interface HostedSnapshot { running: boolean; connection_verified: boolean }
interface Trace {
  operation: string; elapsed_us: number; index_generation: number;
  candidate_count: number; result_count: number; source_count: number;
  estimated_tokens: number; truncated: boolean;
}
interface Inspection { trace: Trace }

export interface HealthWorkspaceHandle {
  refresh(): Promise<void>;
  destroy(): void;
}

/**
 * MP-8D read-only diagnostics. Never display access tokens, source snippets,
 * query results, client identities, filesystem paths or connector secrets.
 */
export function mountHealthWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void): HealthWorkspaceHandle {
  const document = view.ownerDocument;
  let alive = true;
  let version = 0;
  let activeEpoch: number | null = null;
  let refreshing: Promise<void> | null = null;
  view.replaceChildren();
  const panel = document.createElement("section");
  panel.className = "health-workspace";
  panel.setAttribute("aria-label", "Retrieval and connector health");
  const header = document.createElement("header");
  header.className = "health-heading";
  const title = document.createElement("h1");
  title.textContent = "Health & retrieval inspector";
  const close = document.createElement("button");
  close.type = "button"; close.className = "quiet-button"; close.textContent = "Back to archive";
  close.addEventListener("click", onClose);
  header.append(title, close);
  const explanation = document.createElement("p");
  explanation.textContent = "Local diagnostics only. A running tunnel is not proof of remote connectivity. No credentials or retrieved text appear here.";
  const refreshButton = document.createElement("button");
  refreshButton.type = "button"; refreshButton.className = "quiet-button";
  refreshButton.textContent = "Refresh status";
  const status = document.createElement("p");
  status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  const summary = document.createElement("dl");
  summary.className = "health-metrics";
  const form = document.createElement("form");
  form.className = "health-inspection-form";
  const label = document.createElement("label");
  label.textContent = "Inspect retrieval query (metadata only)";
  const input = document.createElement("input");
  input.type = "search"; input.maxLength = 200; input.required = true;
  input.placeholder = "Enter a search phrase";
  label.append(input);
  const run = document.createElement("button");
  run.type = "submit"; run.className = "primary-button"; run.textContent = "Inspect";
  const tracePanel = document.createElement("div");
  tracePanel.className = "health-inspection-result";
  tracePanel.setAttribute("role", "status"); tracePanel.setAttribute("aria-live", "polite");
  form.append(label, run);
  panel.append(header, explanation, refreshButton, status, summary, form, tracePanel);
  view.append(panel);

  function renderMetrics(items: readonly [string, string][]) {
    summary.replaceChildren();
    for (const [labelText, valueText] of items) {
      const dt = document.createElement("dt"); dt.textContent = labelText;
      const dd = document.createElement("dd"); dd.textContent = valueText;
      summary.append(dt, dd);
    }
  }

  const refresh = (): Promise<void> => {
    if (refreshing) return refreshing;
    const requestVersion = ++version;
    refreshButton.disabled = true;
    status.textContent = "Checking live local services…";
    const task = (async () => {
      try {
        const security = await invoke<SecuritySnapshot>("get_security_status");
        if (!alive || requestVersion !== version) return;
        if (security.locked || !security.configured || !Number.isSafeInteger(security.epoch)) {
          activeEpoch = null;
          throw new Error("MP_HEALTH_LOCKED");
        }
        // Never re-use a trace or epoch-scoped status after changing unlock epoch.
        if (activeEpoch !== null && activeEpoch !== security.epoch) tracePanel.replaceChildren();
        activeEpoch = security.epoch;
        const [index, local, hosted] = await Promise.allSettled([
          invoke<IndexSnapshot>("get_archive_index_stats"),
          invoke<LocalSnapshot>("get_local_mcp_status"),
          invoke<HostedSnapshot>("get_hosted_tunnel_status"),
        ]);
        if (!alive || requestVersion !== version) return;
        const after = await invoke<SecuritySnapshot>("get_security_status");
        if (!alive || requestVersion !== version) return;
        if (after.locked || after.epoch !== security.epoch) {
          activeEpoch = null;
          summary.replaceChildren(); tracePanel.replaceChildren();
          throw new Error("MP_HEALTH_LOCKED");
        }
        const metrics: [string, string][] = [];
        if (index.status === "fulfilled" && Number.isSafeInteger(index.value.generation)) {
          const v = index.value;
          metrics.push(["Search index", "Generation " + v.generation]);
          metrics.push(["Sources / revisions / chunks",
            [v.sources, v.revisions, v.chunks].map(n => Number.isSafeInteger(n) && n >= 0 ? n.toLocaleString() : "?").join(" / ")]);
        } else metrics.push(["Search index", "Unavailable (no health inference)"]);
        metrics.push(["Local MCP", local.status === "fulfilled" ? (local.value.running ? "Process running" : "Stopped") : "Unavailable"]);
        metrics.push(["Hosted tunnel", hosted.status !== "fulfilled" ? "Unavailable" :
          !hosted.value.running ? "Stopped" :
            hosted.value.connection_verified ? "Helper running; connector reports verified" :
              "Helper running; connectivity not verified"]);
        renderMetrics(metrics);
        status.textContent = [index, local, hosted].every(x => x.status === "fulfilled")
          ? "Live local status checked." : "Partial status: one or more checks unavailable.";
      } catch {
        if (!alive || requestVersion !== version) return;
        summary.replaceChildren(); tracePanel.replaceChildren();
        status.textContent = "Health unavailable or Mind Place locked. Unlock and retry.";
      } finally {
        if (alive && requestVersion === version) refreshButton.disabled = false;
      }
    })();
    refreshing = task;
    void task.finally(() => { if (refreshing === task) refreshing = null; });
    return task;
  };

  refreshButton.addEventListener("click", () => { void refresh(); });
  form.addEventListener("submit", event => {
    event.preventDefault();
    const query = input.value.trim();
    if (!query || query.length > 200 || activeEpoch === null || run.disabled) return;
    const expectedEpoch = activeEpoch;
    const current = ++version;
    refreshing = null;
    run.disabled = true;
    tracePanel.textContent = "Running bounded metadata inspection…";
    void (async () => {
      try {
        const before = await invoke<SecuritySnapshot>("get_security_status");
        if (!alive || current !== version || before.locked || before.epoch !== expectedEpoch) throw Error("expired");
        const result = await invoke<Inspection>("inspect_retrieval", {
          request: { queries: [query], filters: {}, limit: 10 },
        });
        const after = await invoke<SecuritySnapshot>("get_security_status");
        if (!alive || current !== version || after.locked || after.epoch !== expectedEpoch) throw Error("expired");
        const t = result?.trace;
        if (!t || !Number.isFinite(t.elapsed_us) || !Number.isSafeInteger(t.result_count)) throw Error("bad trace");
        tracePanel.textContent = [
          "Operation: " + t.operation,
          "Index generation: " + t.index_generation,
          "Candidates: " + t.candidate_count,
          "Results: " + t.result_count,
          "Sources: " + t.source_count,
          "Elapsed: " + (t.elapsed_us / 1000).toFixed(1) + " ms",
          "Truncated: " + (t.truncated ? "yes" : "no"),
        ].join(" · ");
      } catch {
        if (alive && current === version) tracePanel.textContent = "Inspection unavailable, failed or session expired. No results disclosed.";
      } finally {
        if (alive && current === version) run.disabled = false;
      }
    })();
  });
  void refresh();
  return {
    refresh,
    destroy: () => {
      alive = false; ++version; activeEpoch = null; refreshing = null;
      input.value = ""; summary.replaceChildren(); tracePanel.replaceChildren();
      if (panel.parentNode === view) view.replaceChildren();
    },
  };
}
