import type { Invoke, ClipboardWriter } from "./reader.js";
interface Status { running: boolean; endpoint: string; }
interface Issued { client_id: string; credential: string; }
interface Registered { id: string; display_name: string; state: string; policy_revision: number; }
interface HostedStatus { running: boolean; tunnel_id: string | null; connection_verified: boolean; }
/** Owner-only UI. MCP clients cannot invoke these commands. */
export function mountLocalMcpSetup(parent: HTMLElement, invoke: Invoke, clipboard: ClipboardWriter): void {
  const section = document.createElement("section");
  section.className = "setup-mcp";
  section.innerHTML = [
    '<h3 class="setup-section-title">Local AI connections (MCP)</h3>',
    '<p>Connect local AI clients to your archive. Access is read-only, loopback-only, and disabled until started after unlock.</p>',
    '<p><code>http://127.0.0.1:43731/mcp</code></p>',
    '<p data-mcp-status role="status" aria-live="polite">Checking status…</p>',
    '<div class="register-actions"><button type="button" class="quiet-button" data-mcp-action="start">Start MCP</button>',
    '<button type="button" class="quiet-button" data-mcp-action="stop">Stop MCP</button></div>',
    '<label class="search-label" for="mcp-client-name">Reader identity</label>',
    '<input id="mcp-client-name" maxlength="128" value="Local AI client" autocomplete="off">',
    '<button type="button" class="primary-button" data-mcp-action="issue">Issue read-only credential</button>',
    '<div data-mcp-secret role="status" aria-live="polite"></div>',
    '<h4>Registered clients</h4>',
    '<div data-mcp-clients role="list"></div>',
    '<button type="button" class="quiet-button" data-mcp-action="more">Load more clients</button>',
    '<p>Use Streamable HTTP with an Authorization Bearer header in your client settings.',
    ' Never commit credentials or put them in a URL. Locking Mind Place stops the listener.</p>',
    '<div class="setup-mcp-hosted">',
    '<h4>Hosted ChatGPT connection (Secure MCP Tunnel)</h4>',
    '<p>This uses the official outbound-only tunnel-client; it does not publish a public MCP URL.',
    ' Queries and selected archive evidence will traverse the OpenAI service when your remote client requests them.</p>',
    '<p>The official tunnel helper is included with Mind Place for Windows x64. No separate download is needed.</p>',
    '<label class="search-label" for="hosted-tunnel-id">Existing OpenAI tunnel ID</label>',
    '<input id="hosted-tunnel-id" spellcheck="false" autocapitalize="off" autocomplete="off"',
    ' placeholder="tunnel_..." maxlength="48">',
    '<label class="search-label" for="hosted-runtime-key">OpenAI runtime API key (not admin key)</label>',
    '<input id="hosted-runtime-key" type="password" autocomplete="off" spellcheck="false">',
    '<label class="search-label"><input type="checkbox" id="hosted-consent">',
    ' I understand a connected hosted AI can request my permitted read-only archive evidence.</label>',
    '<div class="register-actions"><button type="button" class="quiet-button" data-mcp-action="hosted-start">',
    'Start secure tunnel</button><button type="button" class="quiet-button" data-mcp-action="hosted-stop">',
    'Stop tunnel and revoke its credential</button></div>',
    '<p data-hosted-status role="status" aria-live="polite">Checking hosted tunnel…</p>',
    '<p>After starting, use ChatGPT Plugins → Add custom MCP server → Tunnel, then select/paste your tunnel ID.',
    ' This screen reports only whether the helper process is alive; live connectivity must be verified separately.',
    ' Stop or lock Mind Place to close remote read access.</p>',
    '</div>',
  ].join("");
  parent.append(document.createElement("hr"), section);
  const state = section.querySelector<HTMLElement>("[data-mcp-status]")!;
  const secret = section.querySelector<HTMLElement>("[data-mcp-secret]")!;
  const hostedState = section.querySelector<HTMLElement>("[data-hosted-status]")!;
  const refreshHostedStatus = async (): Promise<void> => {
    try {
      const hosted = await invoke<HostedStatus>("get_hosted_tunnel_status");
      if (section.isConnected) {
        hostedState.textContent = hosted.running
          ? "Tunnel helper is running (not yet verified connected). ID: " + (hosted.tunnel_id ?? "unknown")
          : "Hosted tunnel stopped; no remote access is active through this helper.";
      }
    } catch {
      if (section.isConnected) hostedState.textContent = "Could not check hosted tunnel.";
    }
  };
  void refreshHostedStatus();
  void invoke<Status>("get_local_mcp_status").then((result) => {
    if (section.isConnected) state.textContent = result.running ? "Local MCP is running." : "Local MCP is stopped.";
  }).catch(() => {
    if (section.isConnected) state.textContent = "MCP status unavailable.";
  });

  const clientsElement = section.querySelector<HTMLElement>("[data-mcp-clients]")!;
  const more = section.querySelector<HTMLButtonElement>('[data-mcp-action="more"]')!;
  let cursor: string | undefined;
  let listing = false;
  const loadClients = async (reset = false): Promise<void> => {
    if (listing) return;
    listing = true;
    more.disabled = true;
    const nextCursor = reset ? undefined : cursor;
    try {
      const page = await invoke<Registered[]>("list_local_mcp_clients",
        nextCursor ? { after: nextCursor } : {});
      if (!section.isConnected) return;
      if (reset) {
        clientsElement.replaceChildren();
        cursor = undefined;
      }
      for (const client of page) {
        const row = document.createElement("div");
        row.className = "register-actions";
        row.setAttribute("role", "listitem");
        const label = document.createElement("span");
        label.textContent = client.display_name + " (" + client.state + ")";
        row.append(label);
        if (client.state !== "revoked") {
          const revoke = document.createElement("button");
          revoke.type = "button";
          revoke.className = "quiet-button";
          revoke.dataset.mcpAction = "revoke";
          revoke.dataset.clientId = client.id;
          revoke.textContent = "Revoke";
          row.append(revoke);
        }
        clientsElement.append(row);
      }
      if (page.length) cursor = page[page.length - 1].id;
      more.hidden = page.length < 50;
    } catch {
      if (section.isConnected) state.textContent = "Client list unavailable; access remains fail-closed.";
    } finally {
      listing = false;
      if (section.isConnected) more.disabled = false;
    }
  };
  void loadClients(true);
  section.addEventListener("click", async (event) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest<HTMLButtonElement>("[data-mcp-action]");
    if (!button || button.disabled) return;
    button.disabled = true;
    try {
      switch (button.dataset.mcpAction) {
        case "start":
        case "stop": {
          const command = button.dataset.mcpAction === "start" ? "start_local_mcp" : "stop_local_mcp";
          const result = await invoke<Status>(command);
          if (section.isConnected) state.textContent = result.running ? "Local MCP is running." : "Local MCP is stopped.";
          await refreshHostedStatus();
          break;
        }
        case "hosted-start": {
          const id = section.querySelector<HTMLInputElement>("#hosted-tunnel-id")?.value.trim() ?? "";
          const keyInput = section.querySelector<HTMLInputElement>("#hosted-runtime-key");
          const apiKey = keyInput?.value ?? "";
          if (keyInput) keyInput.value = "";
          const acknowledgedRemoteAccess = section.querySelector<HTMLInputElement>("#hosted-consent")?.checked ?? false;
          await invoke<HostedStatus>("start_hosted_tunnel", {
            tunnelId: id,
            apiKey,
            acknowledgedRemoteAccess,
          });
          await refreshHostedStatus();
          break;
        }
        case "hosted-stop": {
          await invoke<HostedStatus>("stop_hosted_tunnel");
          await refreshHostedStatus();
          break;
        }
        case "more": {
          await loadClients();
          break;
        }
        case "issue": {
          const displayName = section.querySelector<HTMLInputElement>("#mcp-client-name")?.value.trim() ?? "";
          const result = await invoke<Issued>("issue_local_mcp_reader", { displayName });
          if (!section.isConnected) break;
          secret.replaceChildren();
          const note = document.createElement("p");
          note.textContent = "Copy this credential now. It is displayed only once.";
          const token = document.createElement("code");
          token.dataset.mcpToken = result.client_id;
          token.textContent = result.credential;
          const copy = document.createElement("button");
          copy.type = "button";
          copy.className = "quiet-button";
          copy.dataset.mcpAction = "copy";
          copy.textContent = "Copy credential";
          const revoke = document.createElement("button");
          revoke.type = "button";
          revoke.className = "quiet-button";
          revoke.dataset.mcpAction = "revoke";
          revoke.dataset.clientId = result.client_id;
          revoke.textContent = "Revoke client";
          secret.append(note, token, copy, revoke);
          void loadClients(true);
          break;
        }
        case "copy": {
          const value = secret.querySelector<HTMLElement>("[data-mcp-token]")?.textContent;
          if (!value) throw new Error("No credential to copy");
          await clipboard.writeText(value);
          if (section.isConnected) state.textContent = "Credential copied. Clear clipboard after setup.";
          break;
        }
        case "revoke": {
          const clientId = button.dataset.clientId;
          if (!clientId) throw new Error("Missing client identity");
          await invoke("revoke_local_mcp_client", { clientId });
          if (section.isConnected) {
            secret.replaceChildren();
            state.textContent = "Client revoked.";
            void loadClients(true);
            await refreshHostedStatus();
          }
          break;
        }
      }
    } catch (error) {
      if (section.isConnected) {
        const message = String(error);
        state.textContent = /MP_TUNNEL_(BINARY|INTEGRITY):/.test(message)
          ? "The bundled tunnel helper is missing or damaged. Repair or reinstall Mind Place, then try again."
          : "MCP operation failed; check local setup, tunnel ID, and runtime key.";
        await refreshHostedStatus();
      }
    } finally {
      if (button.isConnected) button.disabled = false;
    }
  });
}
