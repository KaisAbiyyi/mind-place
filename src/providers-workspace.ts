import type { ClipboardWriter, Invoke } from "./reader.js";
import { mountLocalMcpSetup } from "./mcp-setup.js";
import { createUiButton, createUiCard } from "./ui-primitives.js";

export interface ProvidersHandle { ready: Promise<void>; destroy(): void }
interface LocalStatus { running: boolean }
interface HostedStatus { running: boolean; connection_verified: boolean }

/** Capture adapters, transport processes, registered identities and verified handshakes are distinct facts. */
export function mountProvidersWorkspace(view: HTMLElement, invoke: Invoke, clipboard: ClipboardWriter): ProvidersHandle {
  const doc = view.ownerDocument;
  let alive = true;
  const shell = doc.createElement("section"); shell.className = "providers-workspace";
  const heading = doc.createElement("header"); heading.className = "providers-page-head";
  const title = doc.createElement("h1"); title.textContent = "AI Providers & MCP Authorization";
  const description = doc.createElement("p"); description.textContent = "Browser capture and authorized AI access are separate. Connections start only after your explicit action.";
  heading.append(title, description);
  const grid = doc.createElement("div"); grid.className = "providers-grid";
  for (const [id, name, capture] of [
    ["chatgpt", "ChatGPT", "Manual browser capture; bounded history loading; optional loaded-message automatic saving (off by default)."],
    ["claude", "Claude", "Manual browser capture of loaded messages. Automatic capture and history loading are unavailable."],
    ["gemini", "Gemini · Optional", "Manual browser capture of loaded messages. Automatic capture and history loading are unavailable."],
  ]) {
    const card = createUiCard(doc, { title: name, className: "provider-card" }); card.element.dataset.providerCard = id;
    const status = doc.createElement("p"); status.dataset.providerHandshake = id;
    status.textContent = "MCP client handshake: not verified. Browser adapter support does not establish a connection.";
    const support = doc.createElement("p"); support.textContent = capture;
    const scopeLabel = doc.createElement("label"); scopeLabel.textContent = "Collection access scope";
    const scope = doc.createElement("select"); scope.disabled = true; scope.dataset.providerScope = id;
    scope.setAttribute("aria-label", name + " collection scope");
    const option = doc.createElement("option"); option.textContent = "Per-provider collection grants unavailable"; scope.append(option); scopeLabel.append(scope);
    const write = createUiButton(doc, "Write access unavailable", { disabled: true }); write.dataset.providerWrite = id;
    const gate = doc.createElement("p"); gate.className = "provider-capability-note";
    gate.textContent = "Full-read/write and per-provider scopes require the separately qualified provider authority. This screen cannot grant them.";
    card.body.append(status, support, scopeLabel, write, gate); grid.append(card.element);
  }
  const transport = createUiCard(doc, { title: "Transport & authorization", className: "providers-transport" });
  const local = doc.createElement("p"); local.dataset.providerLocal = ""; local.setAttribute("role", "status"); local.textContent = "Checking local MCP…";
  const hosted = doc.createElement("p"); hosted.dataset.providerHosted = ""; hosted.setAttribute("role", "status"); hosted.textContent = "Checking hosted tunnel…";
  const retry = createUiButton(doc, "Refresh connection status");
  transport.body.append(local, hosted, retry);
  const setup = createUiCard(doc, { title: "Browser capture & imports" });
  const captureNote = doc.createElement("p"); captureNote.textContent = "Register the native host for Chrome, Edge or Brave and load the extension. Manual capture runs in the active browser tab. Generic imports preserve their raw source.";
  const openSetup = createUiButton(doc, "Browser & host setup"); openSetup.dataset.action = "open-setup";
  const imports = createUiButton(doc, "Import a transcript"); imports.dataset.action = "open-import-setup";
  setup.body.append(captureNote, openSetup, imports);
  const audit = createUiCard(doc, { title: "MCP activity & audit" });
  const auditNote = doc.createElement("p"); auditNote.textContent = "No per-provider handshake or tool-call audit feed is exposed by the desktop API. Registered clients below show actual identity state; registration is not proof of a connected session.";
  audit.body.append(auditNote);
  shell.append(heading, grid, transport.element, setup.element, audit.element);
  view.replaceChildren(shell); view.setAttribute("aria-busy", "false");

  const guardedInvoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (!alive) throw new Error("MP_UI_SESSION_EXPIRED");
    const result = await invoke<T>(command, args);
    if (!alive) throw new Error("MP_UI_SESSION_EXPIRED");
    if (["get_local_mcp_status", "start_local_mcp", "stop_local_mcp"].includes(command)) {
      const state = result as LocalStatus;
      local.textContent = typeof state?.running === "boolean"
        ? state.running ? "Local MCP listener running · loopback read-only transport." : "Local MCP listener stopped."
        : "Local MCP status unavailable.";
    }
    if (["get_hosted_tunnel_status", "start_hosted_tunnel", "stop_hosted_tunnel"].includes(command)) {
      const state = result as HostedStatus;
      hosted.textContent = typeof state?.running === "boolean"
        ? state.running ? "Hosted helper running; provider handshake is not verified by this process status." : "Hosted tunnel stopped."
        : "Hosted tunnel status unavailable.";
    }
    return result;
  };
  const mcp = mountLocalMcpSetup(shell, guardedInvoke, { writeText: value => alive ? clipboard.writeText(value) : Promise.reject(new Error("MP_UI_SESSION_EXPIRED")) });
  async function refresh() {
    const results = await Promise.allSettled([guardedInvoke("get_local_mcp_status"), guardedInvoke("get_hosted_tunnel_status")]);
    if (!alive) return;
    if (results[0].status === "rejected") local.textContent = "Local MCP status unavailable. Unlock or retry; no connection was started.";
    if (results[1].status === "rejected") hosted.textContent = "Hosted status unavailable. No connection is assumed.";
  }
  retry.addEventListener("click", () => { if (alive) void refresh(); });
  const ready = refresh();
  return { ready, destroy() { alive = false; mcp.destroy(); for (const input of shell.querySelectorAll("input")) input.value = ""; shell.replaceChildren(); shell.remove(); } };
}
