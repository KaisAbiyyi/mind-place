import type { Invoke, ClipboardWriter } from "./reader.js";
interface Status { running: boolean; endpoint: string; }
interface Issued { client_id: string; credential: string; }
interface Registered { id: string; display_name: string; state: string; policy_revision: number; }
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
  ].join("");
  parent.append(document.createElement("hr"), section);
  const state = section.querySelector<HTMLElement>("[data-mcp-status]")!;
  const secret = section.querySelector<HTMLElement>("[data-mcp-secret]")!;
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
          }
          break;
        }
      }
    } catch {
      if (section.isConnected) state.textContent = "MCP operation failed; access remains fail-closed.";
    } finally {
      if (button.isConnected) button.disabled = false;
    }
  });
}
