import type { Invoke } from "./reader.js";
import type { UiModuleDataState, UiRouteId, UiSurfaceStatus } from "./ui-contracts.js";
import { createUiButton, createUiCard, createUiDialog, createUiStatus, createUiTable } from "./ui-primitives.js";

export type SettingsSection = "general" | "storage-vault" | "privacy" | "diagnostics";

export interface SettingsWorkspaceOptions {
  initialSection?: SettingsSection;
  onLock?: () => void;
  onNavigate?: (route: UiRouteId) => void;
  onOpenSetupDialog?: (tab: "archive" | "documents" | "connections" | "privacy") => void;
  onClose?: () => void;
}

export interface SettingsWorkspaceHandle {
  setSection(section: SettingsSection): void;
  refresh(): Promise<void>;
  state(): UiModuleDataState<unknown>;
  destroy(): void;
}

export interface OverviewHealthSummary {
  securityConfigured: boolean;
  securityLocked: boolean;
  securityEpoch: number;
  indexSources: number;
  indexRevisions: number;
  indexChunks: number;
  indexGeneration: number;
  localMcpRunning: boolean;
  hostedTunnelRunning: boolean;
  hostedTunnelVerified: boolean;
}

interface SecuritySnapshot {
  configured: boolean;
  locked: boolean;
  epoch: number;
}

interface IndexSnapshot {
  sources: number;
  revisions: number;
  chunks: number;
  generation: number;
}

interface LocalMcpSnapshot {
  running: boolean;
}

interface HostedTunnelSnapshot {
  running: boolean;
  connection_verified: boolean;
}

interface Trace {
  operation: string;
  elapsed_us: number;
  index_generation: number;
  candidate_count: number;
  result_count: number;
  source_count: number;
  estimated_tokens: number;
  truncated: boolean;
}

interface Inspection {
  trace: Trace;
}

interface BackupMetadata {
  manifest_path: string;
  total_chunks: number;
  timestamp: string;
}

export async function fetchHealthSummary(invoke: Invoke): Promise<OverviewHealthSummary | null> {
  try {
    const sec = await invoke<SecuritySnapshot>("get_security_status").catch(() => null);
    if (!sec || sec.locked) return null;

    const [indexRes, mcpRes, tunnelRes] = await Promise.allSettled([
      invoke<IndexSnapshot>("get_archive_index_stats"),
      invoke<LocalMcpSnapshot>("get_local_mcp_status"),
      invoke<HostedTunnelSnapshot>("get_hosted_tunnel_status"),
    ]);

    const index = indexRes.status === "fulfilled" ? indexRes.value : { sources: 0, revisions: 0, chunks: 0, generation: 0 };
    const mcp = mcpRes.status === "fulfilled" ? mcpRes.value : { running: false };
    const tunnel = tunnelRes.status === "fulfilled" ? tunnelRes.value : { running: false, connection_verified: false };

    return {
      securityConfigured: sec.configured,
      securityLocked: sec.locked,
      securityEpoch: sec.epoch,
      indexSources: index.sources,
      indexRevisions: index.revisions,
      indexChunks: index.chunks,
      indexGeneration: index.generation,
      localMcpRunning: mcp.running,
      hostedTunnelRunning: tunnel.running,
      hostedTunnelVerified: tunnel.connection_verified,
    };
  } catch {
    return null;
  }
}

/**
 * MP-UI-08: HTML Settings, storage & vault recovery panels, privacy controls
 * and operational diagnostics.
 */
export function mountSettingsWorkspace(
  view: HTMLElement,
  invoke: Invoke,
  options: SettingsWorkspaceOptions = {},
): SettingsWorkspaceHandle {
  const doc = view.ownerDocument;
  let alive = true;
  let activeSection: SettingsSection = options.initialSection ?? "general";
  let surfaceStatus: UiSurfaceStatus = "idle";

  let securityState: SecuritySnapshot | null = null;
  let indexStats: IndexSnapshot | null = null;
  let localMcp: LocalMcpSnapshot | null = null;
  let hostedTunnel: HostedTunnelSnapshot | null = null;
  let lastTrace: Trace | null = null;

  const html = doc.documentElement;

  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string) => {
    const el = doc.createElement(tag);
    if (text !== undefined) el.textContent = text;
    return el;
  };

  const shell = make("section");
  shell.className = "settings-workspace ui-overview-page";
  shell.setAttribute("aria-label", "Settings and operational diagnostics");

  // Header
  const header = make("header");
  header.className = "settings-workspace-header ui-overview-heading";
  const title = make("h1", "Settings");
  const description = make(
    "p",
    "Privacy & security, Provider connections and Operational diagnostics.",
  );
  header.append(title, description);

  // Section tabs nav
  const nav = make("nav");
  nav.className = "settings-sections-nav";
  nav.setAttribute("role", "tablist");
  nav.setAttribute("aria-label", "Settings categories");

  const sectionsList: Array<{ id: SettingsSection; label: string }> = [
    { id: "general", label: "General & Appearance" },
    { id: "storage-vault", label: "Storage & Vault" },
    { id: "privacy", label: "Privacy & security" },
    { id: "diagnostics", label: "Operational diagnostics" },
  ];

  for (const s of sectionsList) {
    const tabBtn = make("button", s.label);
    tabBtn.type = "button";
    tabBtn.role = "tab";
    tabBtn.id = `settings-tab-${s.id}`;
    tabBtn.dataset.settingsSection = s.id;
    tabBtn.setAttribute("aria-controls", `settings-panel-${s.id}`);
    tabBtn.setAttribute("aria-selected", String(s.id === activeSection));
    tabBtn.className = "settings-section-tab";
    nav.append(tabBtn);
  }

  // Content container
  const contentArea = make("div");
  contentArea.className = "settings-content-area";

  // Reusable confirmation dialog
  const confirmDialog = createUiDialog(doc, {
    id: "settings-confirm-dialog",
    title: "Confirm Action",
    description: "Please verify before proceeding with this operation.",
  });
  const confirmDialogBody = confirmDialog.querySelector<HTMLElement>(".ui-dialog-body")!;
  const confirmMsg = make("p");
  confirmMsg.className = "settings-confirm-message";
  const confirmActions = make("div");
  confirmActions.className = "settings-confirm-actions";
  const confirmProceedBtn = createUiButton(doc, "Proceed", { variant: "primary" });
  const confirmCancelBtn = createUiButton(doc, "Cancel", { variant: "quiet" });
  confirmCancelBtn.dataset.action = "close-ui-dialog";
  confirmCancelBtn.dataset.dialogId = "settings-confirm-dialog";
  confirmActions.append(confirmProceedBtn, confirmCancelBtn);
  confirmDialogBody.append(confirmMsg, confirmActions);

  shell.append(header, nav, contentArea, confirmDialog);
  view.replaceChildren(shell);

  function promptConfirm(message: string, onConfirm: () => void) {
    confirmMsg.textContent = message;
    const clickHandler = () => {
      confirmProceedBtn.removeEventListener("click", clickHandler);
      if (typeof confirmDialog.close === "function") confirmDialog.close();
      else confirmDialog.removeAttribute("open");
      onConfirm();
    };
    confirmProceedBtn.addEventListener("click", clickHandler);
    if (typeof confirmDialog.showModal === "function") {
      try {
        confirmDialog.showModal();
      } catch {
        confirmDialog.setAttribute("open", "");
      }
    } else {
      confirmDialog.setAttribute("open", "");
    }
  }

  async function loadDiagnosticsData() {
    surfaceStatus = "loading";
    try {
      const sec = await invoke<SecuritySnapshot>("get_security_status").catch(() => null);
      if (sec) securityState = sec;

      const [indexRes, mcpRes, tunnelRes] = await Promise.allSettled([
        invoke<IndexSnapshot>("get_archive_index_stats"),
        invoke<LocalMcpSnapshot>("get_local_mcp_status"),
        invoke<HostedTunnelSnapshot>("get_hosted_tunnel_status"),
      ]);

      if (indexRes.status === "fulfilled") indexStats = indexRes.value;
      if (mcpRes.status === "fulfilled") localMcp = mcpRes.value;
      if (tunnelRes.status === "fulfilled") hostedTunnel = tunnelRes.value;

      surfaceStatus = "ready";
    } catch {
      surfaceStatus = "error";
    }
  }

  function renderGeneralSection(): HTMLElement {
    const panel = make("div");
    panel.id = "settings-panel-general";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", "settings-tab-general");
    panel.className = "settings-panel";

    const appearanceCard = createUiCard(doc, {
      title: "Display Preferences",
      description: "Adjust visual theme, layout density and technical metadata visibility.",
    });

    // Theme
    const themeRow = make("div");
    themeRow.className = "settings-control-row";
    const themeLabel = make("span", "Color Theme: ");
    const currentTheme = html.dataset.theme === "dark" ? "Dark" : "Light";
    const themeValue = make("strong", currentTheme);
    const themeToggleBtn = createUiButton(doc, `Switch to ${currentTheme === "Light" ? "Dark" : "Light"}`, {
      variant: "secondary",
    });
    themeToggleBtn.dataset.action = "toggle-theme-setting";
    themeRow.append(themeLabel, themeValue, themeToggleBtn);

    // Density
    const densityRow = make("div");
    densityRow.className = "settings-control-row";
    const densityLabel = make("span", "Interface Density: ");
    const currentDensity = html.dataset.density === "compact" ? "Compact" : "Comfortable";
    const densityValue = make("strong", currentDensity);
    const densityToggleBtn = createUiButton(
      doc,
      `Switch to ${currentDensity === "Comfortable" ? "Compact" : "Comfortable"}`,
      { variant: "secondary" },
    );
    densityToggleBtn.dataset.action = "toggle-density-setting";
    densityRow.append(densityLabel, densityValue, densityToggleBtn);

    // Technical Mode
    const techRow = make("div");
    techRow.className = "settings-control-row";
    const techLabel = make("span", "Technical Mode (Provenance & Diagnostic IDs): ");
    const currentTech = html.dataset.technicalMode === "true";
    const techValue = make("strong", currentTech ? "Enabled" : "Disabled");
    const techToggleBtn = createUiButton(doc, currentTech ? "Disable Technical Mode" : "Enable Technical Mode", {
      variant: "secondary",
    });
    techToggleBtn.dataset.action = "toggle-tech-setting";
    techRow.append(techLabel, techValue, techToggleBtn);

    appearanceCard.body.append(themeRow, densityRow, techRow);
    panel.append(appearanceCard.element);
    return panel;
  }

  function renderStorageVaultSection(): HTMLElement {
    const panel = make("div");
    panel.id = "settings-panel-storage-vault";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", "settings-tab-storage-vault");
    panel.className = "settings-panel";

    // Vault Status Card
    const vaultCard = createUiCard(doc, {
      title: "Encrypted Vault & Key Status",
      description: "State of local vault authentication and security epoch lease.",
    });

    const isLocked = securityState ? securityState.locked : false;
    const isConfigured = securityState ? securityState.configured : false;
    const epoch = securityState ? securityState.epoch : 0;

    const statusGrid = make("div");
    statusGrid.className = "settings-status-grid";

    statusGrid.append(
      make("span", "Security Configured:"),
      createUiStatus(doc, isConfigured ? "Configured" : "Unconfigured", isConfigured ? "complete" : "neutral"),
      make("span", "Vault Access:"),
      createUiStatus(doc, isLocked ? "Locked" : "Unlocked", isLocked ? "locked" : "complete"),
      make("span", "Security Epoch:"),
      make("strong", String(epoch)),
    );

    const vaultNote = make(
      "p",
      "Mind Place operates with zero external cloud dependencies. SQLite databases and FTS5 indexes are secured behind your active unlock epoch.",
    );
    vaultNote.className = "list-note";

    vaultCard.body.append(statusGrid, vaultNote);
    panel.append(vaultCard.element);

    // Key Recovery Ceremony Card
    const recoveryCard = createUiCard(doc, {
      title: "Key Recovery Ceremony",
      description: "Authenticate or establish recovery credentials for encrypted vault access.",
    });

    const recoveryControls = make("div");
    recoveryControls.className = "settings-recovery-controls";

    const recoveryStatus = make("p");
    recoveryStatus.className = "list-note settings-recovery-status";

    const secretInput = make("input");
    secretInput.type = "password";
    secretInput.placeholder = "Enter master password / secret";
    secretInput.className = "settings-input";

    const codeInput = make("input");
    codeInput.type = "text";
    codeInput.placeholder = "Enter recovery code (e.g. 24-character export code)";
    codeInput.className = "settings-input";

    const unlockRecoveryBtn = createUiButton(doc, "Unlock with Recovery Code", { variant: "primary" });
    unlockRecoveryBtn.addEventListener("click", async () => {
      const secret = secretInput.value.trim();
      const code = codeInput.value.trim();
      if (!secret || !code) {
        recoveryStatus.textContent = "Please enter both secret and recovery code.";
        return;
      }
      recoveryStatus.textContent = "Attempting recovery unlock…";
      try {
        const res = await invoke<SecuritySnapshot>("unlock_vault_with_recovery", { secret, code });
        securityState = res;
        recoveryStatus.textContent = "Vault unlocked successfully via recovery code.";
        renderSection();
      } catch (err: any) {
        recoveryStatus.textContent = "Recovery failed: " + (err?.message || String(err));
      }
    });

    const beginSetupBtn = createUiButton(doc, "Begin First-Run Vault Ceremony", { variant: "secondary" });
    beginSetupBtn.addEventListener("click", async () => {
      const secret = secretInput.value.trim();
      if (!secret) {
        recoveryStatus.textContent = "Enter secret to prepare vault setup.";
        return;
      }
      recoveryStatus.textContent = "Preparing vault ceremony…";
      try {
        const code = await invoke<string>("begin_vault_setup", { secret });
        recoveryStatus.textContent = `Generated recovery code: ${code}. Save this code in a secure location, then confirm below.`;
        codeInput.value = code;
      } catch (err: any) {
        recoveryStatus.textContent = "Setup error: " + (err?.message || String(err));
      }
    });

    const confirmSetupBtn = createUiButton(doc, "Confirm Recovery Setup", { variant: "secondary" });
    confirmSetupBtn.addEventListener("click", () => {
      const code = codeInput.value.trim();
      if (!code) {
        recoveryStatus.textContent = "Enter generated recovery code to confirm.";
        return;
      }
      promptConfirm("Confirm provisioning this encrypted vault with your recovery code? This action is permanent.", async () => {
        try {
          await invoke("confirm_vault_setup", { code });
          recoveryStatus.textContent = "Vault setup confirmed. Security locked as required by first-run protocol.";
          await loadDiagnosticsData();
          renderSection();
        } catch (err: any) {
          recoveryStatus.textContent = "Confirmation error: " + (err?.message || String(err));
        }
      });
    });

    recoveryControls.append(secretInput, codeInput, unlockRecoveryBtn, beginSetupBtn, confirmSetupBtn);
    recoveryCard.body.append(recoveryControls, recoveryStatus);
    panel.append(recoveryCard.element);

    // Online Vault Backup Card
    const backupCard = createUiCard(doc, {
      title: "Online Vault Backup",
      description: "Create an authenticated atomic snapshot of the SQLCipher database and WAL.",
    });

    const backupForm = make("div");
    backupForm.className = "settings-backup-form";

    const destLabel = make("label", "Absolute Backup Destination Path: ");
    const destInput = make("input");
    destInput.type = "text";
    destInput.placeholder = "e.g. D:\\backups\\mind-place";
    destInput.className = "settings-input";
    destLabel.append(destInput);

    const genLabel = make("label", "Target Generation: ");
    const genInput = make("input");
    genInput.type = "number";
    genInput.min = "1";
    genInput.value = "1";
    genInput.className = "settings-input";
    genLabel.append(genInput);

    const backupStatus = make("p");
    backupStatus.className = "list-note settings-backup-status";

    const createBackupBtn = createUiButton(doc, "Create Online Backup", { variant: "primary" });
    createBackupBtn.addEventListener("click", () => {
      const destination = destInput.value.trim();
      const generation = parseInt(genInput.value, 10);
      if (!destination || !Number.isSafeInteger(generation) || generation < 1) {
        backupStatus.textContent = "Please provide an absolute path and a valid positive generation number.";
        return;
      }
      promptConfirm(`Write online backup to ${destination} for generation ${generation}?`, async () => {
        backupStatus.textContent = "Creating online backup snapshot…";
        try {
          const meta = await invoke<BackupMetadata>("create_vault_online_backup", { destination, generation });
          backupStatus.textContent = `Backup created successfully at ${meta.manifest_path} (${meta.total_chunks} chunks).`;
        } catch (err: any) {
          backupStatus.textContent = "Backup error: " + (err?.message || String(err));
        }
      });
    });

    backupForm.append(destLabel, genLabel, createBackupBtn);
    backupCard.body.append(backupForm, backupStatus);
    panel.append(backupCard.element);

    // Portability & Deletion Boundaries Card
    const boundariesCard = createUiCard(doc, {
      title: "Portability & Privacy Deletion Boundaries",
      description: "Factual boundaries for export, import, and permanent privacy erasure.",
    });

    const boundaryList = make("ul");
    boundaryList.className = "settings-boundary-list";

    const item1 = make("li", "Markdown Export: Transcripts can be exported to standard Markdown via the conversation reader.");
    const item2 = make("li", "Generic Import: Supports JSON, Markdown, and HTML transcripts up to 8 MiB with raw capture retention.");
    const item3 = make(
      "li",
      "Destructive Redaction / Sector Zeroing: Physical raw byte erasure requires MP-9C backend acceptance. Curated Memory forget actions apply non-disclosure tombstones to protect query results without corrupting immutable historical revisions.",
    );

    boundaryList.append(item1, item2, item3);
    boundariesCard.body.append(boundaryList);
    panel.append(boundariesCard.element);

    return panel;
  }

  function renderPrivacySection(): HTMLElement {
    const panel = make("div");
    panel.id = "settings-panel-privacy";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", "settings-tab-privacy");
    panel.className = "settings-panel";

    const sessionCard = createUiCard(doc, {
      title: "Session Control & Active Lease",
      description: "Immediately invalidate current reader authorization and revoke MCP access.",
    });

    const lockBtn = createUiButton(doc, "Lock Mind Place Now", { variant: "primary" });
    lockBtn.dataset.action = "lock-mind-place";
    lockBtn.addEventListener("click", () => {
      options.onLock?.();
    });

    const leaseInfo = make(
      "p",
      `Current Unlock Epoch: ${securityState ? securityState.epoch : 0}. Locking immediately destroys memory keys and shuts down loopback MCP services.`,
    );
    leaseInfo.className = "list-note";

    sessionCard.body.append(lockBtn, leaseInfo);
    panel.append(sessionCard.element);

    const principlesCard = createUiCard(doc, {
      title: "Security & Zero-Telemetry Architecture",
      description: "Local data sovereignty and strict permissions boundary.",
    });

    const principlesList = make("ul");
    principlesList.className = "settings-boundary-list";
    principlesList.append(
      make("li", "Local Execution: All indexing, FTS5 tokenization, and vector models run 100% on this computer."),
      make("li", "Loopback Transport: Local MCP transport binds strictly to 127.0.0.1 with per-client authentication."),
      make("li", "Outbound Tunnel: Hosted tunnel is strictly outbound-only and requires explicit owner authorization."),
      make("li", "Zero Telemetry: No keystrokes, queries, transcripts, or personal corpus contents are transmitted."),
    );
    principlesCard.body.append(principlesList);
    panel.append(principlesCard.element);

    return panel;
  }

  function renderDiagnosticsSection(): HTMLElement {
    const panel = make("div");
    panel.id = "settings-panel-diagnostics";
    panel.setAttribute("role", "tabpanel");
    panel.setAttribute("aria-labelledby", "settings-tab-diagnostics");
    panel.className = "settings-panel";

    // Refresh action row
    const actionRow = make("div");
    actionRow.className = "settings-diagnostics-action-row";
    const refreshDiagBtn = createUiButton(doc, "Refresh Diagnostic Metrics", { variant: "secondary" });
    refreshDiagBtn.addEventListener("click", async () => {
      await loadDiagnosticsData();
      renderSection();
    });
    actionRow.append(refreshDiagBtn);
    panel.append(actionRow);

    // Search Index Card
    const indexCard = createUiCard(doc, {
      title: "Search Index & Corpus Statistics",
      description: "Derived FTS5 index generation and chunk volume.",
    });

    const indexGrid = make("div");
    indexGrid.className = "settings-status-grid";

    const gen = indexStats ? indexStats.generation : 0;
    const sources = indexStats ? indexStats.sources : 0;
    const revisions = indexStats ? indexStats.revisions : 0;
    const chunks = indexStats ? indexStats.chunks : 0;

    indexGrid.append(
      make("span", "Index Generation:"),
      make("strong", `Generation ${gen}`),
      make("span", "Indexed Sources:"),
      make("strong", sources.toLocaleString()),
      make("span", "Archived Revisions:"),
      make("strong", revisions.toLocaleString()),
      make("span", "Search Chunks:"),
      make("strong", chunks.toLocaleString()),
    );

    const rebuildStatus = make("p");
    rebuildStatus.className = "list-note settings-rebuild-status";

    const rebuildBtn = createUiButton(doc, "Rebuild Archive Index", { variant: "secondary" });
    rebuildBtn.addEventListener("click", () => {
      promptConfirm(
        "Rebuilding the archive index will re-scan all archived conversations and rebuild FTS5 search tables. Continue?",
        async () => {
          rebuildStatus.textContent = "Rebuilding archive index…";
          try {
            const stats = await invoke<IndexSnapshot>("rebuild_archive_index");
            indexStats = stats;
            rebuildStatus.textContent = `Index successfully rebuilt. New generation: ${stats.generation} (${stats.chunks} chunks).`;
            renderSection();
          } catch (err: any) {
            rebuildStatus.textContent = "Rebuild error: " + (err?.message || String(err));
          }
        },
      );
    });

    indexCard.body.append(indexGrid, rebuildBtn, rebuildStatus);
    panel.append(indexCard.element);

    // Local MCP & Hosted Tunnel Card
    const mcpCard = createUiCard(doc, {
      title: "Connector & Transport Health",
      description: "Status of local loopback MCP server and optional hosted tunnel.",
    });

    const mcpGrid = make("div");
    mcpGrid.className = "settings-status-grid";

    const mcpRunning = localMcp ? localMcp.running : false;
    const tunnelRunning = hostedTunnel ? hostedTunnel.running : false;
    const tunnelVerified = hostedTunnel ? hostedTunnel.connection_verified : false;

    mcpGrid.append(
      make("span", "Local MCP Server:"),
      createUiStatus(doc, mcpRunning ? "Process running" : "Stopped", mcpRunning ? "complete" : "neutral"),
      make("span", "Hosted Tunnel:"),
      createUiStatus(
        doc,
        !tunnelRunning
          ? "Stopped"
          : tunnelVerified
            ? "Helper running; connector reports verified"
            : "Helper running; connectivity not verified",
        tunnelRunning ? (tunnelVerified ? "complete" : "partial") : "neutral",
      ),
    );

    const mcpNote = make(
      "p",
      "Local MCP binds to loopback only. Running tunnel process does not imply verified remote connectivity. Access tokens are never exposed in diagnostics.",
    );
    mcpNote.className = "list-note";

    mcpCard.body.append(mcpGrid, mcpNote);
    panel.append(mcpCard.element);

    // Retrieval Query Tracer Card
    const tracerCard = createUiCard(doc, {
      title: "Retrieval Query Tracer (Metadata Only)",
      description: "Inspect live query execution latency and candidate evaluation without exposing raw text.",
    });

    const tracerForm = make("form");
    tracerForm.className = "settings-tracer-form";

    const queryInput = make("input");
    queryInput.type = "search";
    queryInput.maxLength = 200;
    queryInput.placeholder = "Enter test retrieval query";
    queryInput.className = "settings-input";

    const inspectBtn = createUiButton(doc, "Inspect Query", { variant: "primary", type: "submit" });
    tracerForm.append(queryInput, inspectBtn);

    const traceResultPanel = make("div");
    traceResultPanel.className = "settings-trace-result-panel";

    tracerForm.addEventListener("submit", async (e) => {
      e.preventDefault();
      const q = queryInput.value.trim();
      if (!q) return;

      inspectBtn.disabled = true;
      traceResultPanel.textContent = "Running bounded metadata inspection…";

      try {
        const res = await invoke<Inspection>("inspect_retrieval", {
          request: { queries: [{ query: q, weight: 1.0 }], filters: {}, limit: 10 },
        });
        const t = res.trace;
        lastTrace = t;

        traceResultPanel.replaceChildren();

        const table = createUiTable(doc, {
          caption: "Live Diagnostic Trace Metrics",
          columns: ["Metric", "Value"],
          rows: [
            ["Operation", t.operation],
            ["Elapsed Latency", `${t.elapsed_us} µs`],
            ["Index Generation", String(t.index_generation)],
            ["Candidates Evaluated", String(t.candidate_count)],
            ["Results Returned", String(t.result_count)],
            ["Sources Examined", String(t.source_count)],
            ["Estimated Tokens", String(t.estimated_tokens)],
            ["Truncated", t.truncated ? "Yes" : "No"],
          ],
        });

        traceResultPanel.append(table);
      } catch (err: any) {
        traceResultPanel.textContent = "Inspection failed: " + (err?.message || "Error");
      } finally {
        inspectBtn.disabled = false;
      }
    });

    tracerCard.body.append(tracerForm, traceResultPanel);
    panel.append(tracerCard.element);

    return panel;
  }

  function renderSection() {
    contentArea.replaceChildren();
    for (const s of sectionsList) {
      const tabBtn = doc.getElementById(`settings-tab-${s.id}`) as HTMLButtonElement | null;
      if (tabBtn) tabBtn.setAttribute("aria-selected", String(s.id === activeSection));
    }

    let panel: HTMLElement;
    switch (activeSection) {
      case "general":
        panel = renderGeneralSection();
        break;
      case "storage-vault":
        panel = renderStorageVaultSection();
        break;
      case "privacy":
        panel = renderPrivacySection();
        break;
      case "diagnostics":
        panel = renderDiagnosticsSection();
        break;
    }
    contentArea.append(panel);
  }

  const clickHandler = (event: Event) => {
    const target = event.target;
    if (!alive || !(target instanceof doc.defaultView!.Element)) return;

    const tab = target.closest<HTMLButtonElement>("[data-settings-section]");
    if (tab?.dataset.settingsSection) {
      activeSection = tab.dataset.settingsSection as SettingsSection;
      renderSection();
      return;
    }

    const actionBtn = target.closest<HTMLElement>("[data-action]");
    if (actionBtn) {
      const act = actionBtn.dataset.action;
      if (act === "toggle-theme-setting") {
        const nextTheme = html.dataset.theme === "dark" ? "light" : "dark";
        html.dataset.theme = nextTheme;
        const themeBtn = doc.querySelector<HTMLButtonElement>("[data-action='toggle-theme']");
        if (themeBtn) {
          themeBtn.textContent = "Theme: " + (nextTheme === "light" ? "Light" : "Dark");
          themeBtn.setAttribute("aria-pressed", String(nextTheme === "dark"));
        }
        renderSection();
      } else if (act === "toggle-density-setting") {
        const nextDensity = html.dataset.density === "compact" ? "comfortable" : "compact";
        html.dataset.density = nextDensity;
        const densityBtn = doc.querySelector<HTMLButtonElement>("[data-action='toggle-density']");
        if (densityBtn) {
          densityBtn.textContent = "Density: " + (nextDensity === "comfortable" ? "Comfortable" : "Compact");
          densityBtn.setAttribute("aria-pressed", String(nextDensity === "compact"));
        }
        renderSection();
      } else if (act === "toggle-tech-setting") {
        const nextTech = html.dataset.technicalMode !== "true";
        html.dataset.technicalMode = String(nextTech);
        const techBtn = doc.querySelector<HTMLButtonElement>("[data-action='toggle-technical']");
        if (techBtn) {
          techBtn.textContent = "Technical: " + (nextTech ? "On" : "Off");
          techBtn.setAttribute("aria-pressed", String(nextTech));
        }
        renderSection();
      } else if (act === "close-ui-dialog") {
        const dialogId = actionBtn.dataset.dialogId;
        if (dialogId === "settings-confirm-dialog") {
          if (typeof confirmDialog.close === "function") confirmDialog.close();
          else confirmDialog.removeAttribute("open");
        }
      }
    }
  };

  shell.addEventListener("click", clickHandler);

  // Synchronous initial render
  renderSection();

  // Initial load
  void loadDiagnosticsData().then(() => {
    if (alive) renderSection();
  });

  return {
    setSection: (section: SettingsSection) => {
      activeSection = section;
      renderSection();
    },
    refresh: async () => {
      await loadDiagnosticsData();
      if (alive) renderSection();
    },
    state: () => ({
      status: surfaceStatus,
      data: { securityState, indexStats, localMcp, hostedTunnel, lastTrace },
      error: null,
      permission: "granted",
      vault: securityState?.locked ? "locked" : "unlocked",
    }),
    destroy: () => {
      alive = false;
      shell.removeEventListener("click", clickHandler);
      shell.replaceChildren();
      view.replaceChildren();
    },
  };
}
