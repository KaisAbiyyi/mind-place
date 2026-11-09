import type { Invoke, ClipboardWriter } from "./reader.js";
interface Status { running: boolean; endpoint: string; }
interface Issued { client_id: string; credential: string; }
interface Registered { id: string; display_name: string; state: string; policy_revision: number; }
interface Collection { id: string; name: string; source_count: number; }
const SOURCE_ID = /^src:v1:[0-9a-f]{64}$/;
interface HostedStatus { running: boolean; tunnel_id: string | null; connection_verified: boolean; }
/** Owner-only UI. MCP clients cannot invoke these commands. */
export function mountLocalMcpSetup(parent: HTMLElement, invoke: Invoke, clipboard: ClipboardWriter): { destroy(): void } {
  const doc = parent.ownerDocument;
  let alive = true;
  const connected = () => alive && section.isConnected;
  const section = doc.createElement("section");
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
    '<details class="setup-mcp-collections" data-mcp-collections>',
    '<summary>Read access: collections</summary>',
    '<p>A reader can read only the conversations in collections granted to it. A new credential reads nothing until you grant a collection.</p>',
    '<label class="search-label" for="mcp-collection-name">Collection name</label>',
    '<input id="mcp-collection-name" maxlength="128" autocomplete="off">',
    '<label class="search-label" for="mcp-collection-sources">Source IDs, one per line (copy each “Source: src:v1:…” from Search results)</label>',
    '<textarea id="mcp-collection-sources" rows="4" spellcheck="false" autocomplete="off"></textarea>',
    '<button type="button" class="quiet-button" data-mcp-action="collection-create">Create collection</button>',
    '<h4>Collections</h4>',
    '<div data-mcp-collection-list role="list"></div>',
    '<label class="search-label" for="mcp-grant-client">Reader</label>',
    '<select id="mcp-grant-client"></select>',
    '<label class="search-label" for="mcp-grant-collection">Collection</label>',
    '<select id="mcp-grant-collection"></select>',
    '<div class="register-actions"><button type="button" class="quiet-button" data-mcp-action="collection-grant">Grant read access</button>',
    '<button type="button" class="quiet-button" data-mcp-action="collection-revoke">Revoke this grant</button></div>',
    '<p data-mcp-grants role="status" aria-live="polite"></p>',
    '<p data-mcp-collection-status role="status" aria-live="polite"></p>',
    '</details>',
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
  parent.append(doc.createElement("hr"), section);
  const state = section.querySelector<HTMLElement>("[data-mcp-status]")!;
  const secret = section.querySelector<HTMLElement>("[data-mcp-secret]")!;
  const hostedState = section.querySelector<HTMLElement>("[data-hosted-status]")!;
  const refreshHostedStatus = async (): Promise<void> => {
    try {
      const hosted = await invoke<HostedStatus>("get_hosted_tunnel_status");
      if (connected()) {
        hostedState.textContent = hosted.running
          ? "Tunnel helper is running (not yet verified connected). ID: " + (hosted.tunnel_id ?? "unknown")
          : "Hosted tunnel stopped; no remote access is active through this helper.";
      }
    } catch {
      if (connected()) hostedState.textContent = "Could not check hosted tunnel.";
    }
  };
  void refreshHostedStatus();
  void invoke<Status>("get_local_mcp_status").then((result) => {
    if (connected()) state.textContent = result.running ? "Local MCP is running." : "Local MCP is stopped.";
  }).catch(() => {
    if (connected()) state.textContent = "MCP status unavailable.";
  });

  const collectionList = section.querySelector<HTMLElement>("[data-mcp-collection-list]")!;
  const collectionStatus = section.querySelector<HTMLElement>("[data-mcp-collection-status]")!;
  const grantsLine = section.querySelector<HTMLElement>("[data-mcp-grants]")!;
  const grantClient = section.querySelector<HTMLSelectElement>("#mcp-grant-client")!;
  const grantCollection = section.querySelector<HTMLSelectElement>("#mcp-grant-collection")!;
  let collections: Collection[] = [];
  const showGrants = async (): Promise<void> => {
    grantsLine.textContent = "";
    if (!grantClient.value) return;
    try {
      const ids = await invoke<string[]>("list_local_mcp_client_collections", { clientId: grantClient.value });
      if (!connected() || !Array.isArray(ids)) return;
      const names = ids.map((id) => collections.find((c) => c.id === id)?.name ?? "unknown collection");
      grantsLine.textContent = names.length ? "This reader can read: " + names.join(", ") : "This reader has no collections and can read nothing.";
    } catch {
      if (connected()) grantsLine.textContent = "Grants unavailable; access remains fail-closed.";
    }
  };
  const loadCollections = async (): Promise<void> => {
    try {
      const [list, readers] = await Promise.all([
        invoke<Collection[]>("list_local_mcp_collections"),
        invoke<Registered[]>("list_local_mcp_clients", {}),
      ]);
      if (!connected() || !Array.isArray(list) || !Array.isArray(readers)) return;
      collections = list;
      collectionList.replaceChildren();
      for (const item of list) {
        const row = doc.createElement("div");
        row.className = "register-actions";
        row.setAttribute("role", "listitem");
        const label = doc.createElement("span");
        label.textContent = item.name + " (" + item.source_count + " conversations)";
        const remove = doc.createElement("button");
        remove.type = "button";
        remove.className = "quiet-button";
        remove.dataset.mcpAction = "collection-delete";
        remove.dataset.collectionId = item.id;
        remove.textContent = "Delete";
        row.append(label, remove);
        collectionList.append(row);
      }
      const fill = (select: HTMLSelectElement, entries: Array<[string, string]>): void => {
        const previous = select.value;
        select.replaceChildren();
        for (const [value, text] of entries) {
          const option = doc.createElement("option");
          option.value = value;
          option.textContent = text;
          select.append(option);
        }
        if (entries.some(([value]) => value === previous)) select.value = previous;
      };
      fill(grantCollection, list.map((c) => [c.id, c.name]));
      fill(grantClient, readers.filter((r) => r.state !== "revoked").map((r) => [r.id, r.display_name]));
      await showGrants();
    } catch {
      if (connected()) collectionStatus.textContent = "Collections unavailable; access remains fail-closed.";
    }
  };
  section.querySelector<HTMLDetailsElement>("[data-mcp-collections]")!.addEventListener("toggle", (event) => {
    if ((event.target as HTMLDetailsElement).open) void loadCollections();
  });
  grantClient.addEventListener("change", () => { void showGrants(); });
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
      if (!connected()) return;
      if (reset) {
        clientsElement.replaceChildren();
        cursor = undefined;
      }
      for (const client of page) {
        const row = doc.createElement("div");
        row.className = "register-actions";
        row.setAttribute("role", "listitem");
        const label = doc.createElement("span");
        label.textContent = client.display_name + " (" + client.state + ")";
        row.append(label);
        if (client.state !== "revoked") {
          const revoke = doc.createElement("button");
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
      if (connected()) state.textContent = "Client list unavailable; access remains fail-closed.";
    } finally {
      listing = false;
      if (connected()) more.disabled = false;
    }
  };
  void loadClients(true);
  section.addEventListener("click", async (event) => {
    const target = event.target;
    if (!(target instanceof doc.defaultView!.Element)) return;
    const button = target.closest<HTMLButtonElement>("[data-mcp-action]");
    if (!connected() || !button || button.disabled) return;
    button.disabled = true;
    try {
      switch (button.dataset.mcpAction) {
        case "start":
        case "stop": {
          const command = button.dataset.mcpAction === "start" ? "start_local_mcp" : "stop_local_mcp";
          const result = await invoke<Status>(command);
          if (connected()) state.textContent = result.running ? "Local MCP is running." : "Local MCP is stopped.";
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
        case "collection-create": {
          const name = section.querySelector<HTMLInputElement>("#mcp-collection-name")?.value.trim() ?? "";
          const lines = (section.querySelector<HTMLTextAreaElement>("#mcp-collection-sources")?.value ?? "")
            .split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
          const unique = [...new Set(lines)];
          if (!name || unique.length > 2048 || unique.some((id) => !SOURCE_ID.test(id))) {
            collectionStatus.textContent = "Enter a name and valid source IDs (src:v1: followed by 64 hex characters); nothing was created.";
            break;
          }
          await invoke<string>("create_local_mcp_collection", { name, sourceIds: unique });
          collectionStatus.textContent = "Collection created. It grants nothing until you grant it to a reader.";
          await loadCollections();
          break;
        }
        case "collection-grant":
        case "collection-revoke": {
          if (!grantClient.value || !grantCollection.value) {
            collectionStatus.textContent = "Choose a reader and a collection first.";
            break;
          }
          const grant = button.dataset.mcpAction === "collection-grant";
          await invoke(grant ? "grant_local_mcp_collection" : "revoke_local_mcp_collection_grant",
            { clientId: grantClient.value, collectionId: grantCollection.value });
          collectionStatus.textContent = grant ? "Read access granted." : "Read access revoked.";
          await showGrants();
          break;
        }
        case "collection-delete": {
          const collectionId = button.dataset.collectionId;
          if (!collectionId) throw new Error("Missing collection identity");
          await invoke("delete_local_mcp_collection", { collectionId });
          collectionStatus.textContent = "Collection deleted and removed from every reader.";
          await loadCollections();
          break;
        }
        case "issue": {
          const displayName = section.querySelector<HTMLInputElement>("#mcp-client-name")?.value.trim() ?? "";
          const result = await invoke<Issued>("issue_local_mcp_reader", { displayName });
          if (!connected()) break;
          secret.replaceChildren();
          const note = doc.createElement("p");
          note.textContent = "Copy this credential now. It is displayed only once.";
          const token = doc.createElement("code");
          token.dataset.mcpToken = result.client_id;
          token.textContent = result.credential;
          const copy = doc.createElement("button");
          copy.type = "button";
          copy.className = "quiet-button";
          copy.dataset.mcpAction = "copy";
          copy.textContent = "Copy credential";
          const revoke = doc.createElement("button");
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
          if (connected()) state.textContent = "Credential copied. Clear clipboard after setup.";
          break;
        }
        case "revoke": {
          const clientId = button.dataset.clientId;
          if (!clientId) throw new Error("Missing client identity");
          await invoke("revoke_local_mcp_client", { clientId });
          if (connected()) {
            secret.replaceChildren();
            state.textContent = "Client revoked.";
            void loadClients(true);
            await refreshHostedStatus();
          }
          break;
        }
      }
    } catch (error) {
      if (connected()) {
        const message = String(error);
        if (button.dataset.mcpAction?.startsWith("collection-")) {
          collectionStatus.textContent = "Collection operation failed; nothing was granted or changed unless shown above.";
          return;
        }
        state.textContent = /MP_TUNNEL_(BINARY|INTEGRITY):/.test(message)
          ? "The bundled tunnel helper is missing or damaged. Repair or reinstall Mind Place, then try again."
          : "MCP operation failed; check local setup, tunnel ID, and runtime key.";
        await refreshHostedStatus();
      }
    } finally {
      if (button.isConnected) button.disabled = false;
    }
  });
  return { destroy() { alive = false; secret.replaceChildren(); for (const input of section.querySelectorAll("input")) { input.value = ""; input.checked = false; } section.replaceChildren(); section.remove(); } };
}
