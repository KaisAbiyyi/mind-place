import type { Invoke, ClipboardWriter } from "./reader.js";
import type { Conversation } from "./contracts.js";
import type { DocumentSearchHit, DocumentPreview } from "./document-roots.js";
import type { UiModuleDataState, UiSurfaceStatus } from "./ui-contracts.js";
import { createUiButton, createUiCard, createUiDialog, createUiDrawer, createUiStatus, createUiTable, createUiStatePanel } from "./ui-primitives.js";

export interface ArchiveSearchHit {
  source_id: string;
  archive_id: string;
  snapshot_id: string;
  message_id: string;
  kind: string;
  role: string;
  title: string;
  snippet: string;
  score: number;
}

export interface QueryTerm {
  query: string;
  weight: number;
}

export interface RetrievalFilters {
  providers?: string[];
  roles?: string[];
  kinds?: string[];
  captured_to?: string;
}

export type ContextPreset = "fast" | "balanced" | "source_first" | "challenge" | "historical";

export interface ContextBudget {
  max_estimated_tokens?: number;
}

export interface ContextRequest {
  queries: QueryTerm[];
  filters: RetrievalFilters;
  preset: ContextPreset;
  as_of?: string | null;
  max_sources: number;
  max_excerpts: number;
  max_per_source: number;
  budget: ContextBudget;
}

export interface ContextExcerpt {
  kind: string;
  message_id: string;
  message_index?: number;
  role: string;
  text: string;
  byte_start: number;
  byte_end: number;
  total_bytes: number;
  score: number;
  matched_queries: number[];
}

export interface ContextGroup {
  source_id: string;
  archive_id: string;
  snapshot_id: string;
  conversation_id: string;
  title: string;
  source_url: string;
  captured_at: string;
  completeness: string;
  indexed_is_current: boolean;
  excerpts: ContextExcerpt[];
}

export interface ContextPack {
  groups: ContextGroup[];
  estimated_tokens: number;
  truncated: boolean;
}

export interface ScoreContribution {
  query_index: number;
  weight: number;
  bm25: number;
  lexical_relevance: number;
  exact_bonus: number;
  weighted_score: number;
}

export interface RetrievalHit {
  source_id: string;
  archive_id: string;
  snapshot_id: string;
  conversation_id: string;
  provider: string;
  source_url: string;
  captured_at: string;
  completeness: string;
  message_id: string;
  part: number;
  kind: string;
  role: string;
  model?: string;
  title: string;
  snippet: string;
  score: number;
  matched_queries: number[];
  exact_queries: number[];
  score_contributions: ScoreContribution[];
}

export interface RetrievalResponse {
  hits: RetrievalHit[];
  candidate_count: number;
  truncated: boolean;
}

export interface RetrievalTrace {
  operation: string;
  elapsed_us: number;
  index_generation: number;
  candidate_count: number;
  result_count: number;
  source_count: number;
  estimated_tokens: number;
  truncated: boolean;
}

export interface SearchInspection {
  trace: RetrievalTrace;
  result: RetrievalResponse;
}

export interface ContextInspection {
  trace: RetrievalTrace;
  result: ContextPack;
}

interface SecuritySnapshot {
  configured: boolean;
  locked: boolean;
  epoch: number;
}

export interface SearchWorkspaceOptions {
  clipboard?: ClipboardWriter;
  onClose?: () => void;
  onSelectConversation?: (id: string, snapshotId?: string) => void;
}

export interface SearchWorkspaceHandle {
  state(): UiModuleDataState<unknown>;
  refresh?(): Promise<void>;
  destroy(): void;
}

type Filter = "all" | "conversations" | "files" | "retrieval";

const DEFAULT_PAGE_SIZE = 10;

/**
 * MP-UI-07: Full retrieval workspace, multi-query filters/policies,
 * progressive evidence fetch, context builder and technical inspector.
 */
export function mountSearchWorkspace(
  view: HTMLElement,
  nativeInvoke: Invoke,
  options: SearchWorkspaceOptions = {},
): SearchWorkspaceHandle {
  const doc = view.ownerDocument;
  let alive = true;
  let epoch = 0;
  let previewEpoch = 0;
  let filter: Filter = "all";
  let multiQueryMode = false;

  let queryTerms: QueryTerm[] = [{ query: "", weight: 1.0 }];
  let selectedPreset: ContextPreset = "balanced";
  let maxSources = 5;
  let maxExcerpts = 10;
  let maxPerSource = 3;
  let tokenBudget = 4000;
  let asOfDate = "";
  let selectedProvider = "";
  let selectedRole = "";
  let selectedKind = "";

  let archiveHits: ArchiveSearchHit[] = [];
  let fileHits: DocumentSearchHit[] = [];
  let retrievalHits: RetrievalHit[] = [];
  let lastContextPack: ContextPack | null = null;
  let lastTrace: RetrievalTrace | null = null;
  let lastScoreContributions: ScoreContribution[] = [];

  let currentPage = 1;
  let pageSize = DEFAULT_PAGE_SIZE;

  let surfaceStatus: UiSurfaceStatus = "idle";

  let securityEpoch: number | null = null;
  const clearEvidence = () => {
    archiveHits = []; fileHits = []; retrievalHits = [];
    lastContextPack = null; lastTrace = null; lastScoreContributions = [];
  };
  const invoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
    if (!alive) throw new Error("MP_UI_SESSION_EXPIRED");
    const before = await checkSecurity();
    const result = await nativeInvoke<T>(command, args);
    const after = await checkSecurity();
    if (!alive || before.epoch !== after.epoch) throw new Error("MP_SECURITY_LOCKED");
    return result;
  };

  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string) => {
    const el = doc.createElement(tag);
    if (text !== undefined) el.textContent = text;
    return el;
  };

  const shell = make("section");
  shell.className = "knowledge-library search-workspace";
  shell.setAttribute("aria-label", "Search your library and context builder");

  const header = make("header");
  header.className = "search-workspace-header";

  const heading = make("h1", "Search your library");
  const note = make(
    "p",
    "Search saved message indexes and currently authorized document roots. Matches are bounded; this is not a full archive listing.",
  );
  note.className = "list-note";

  const closeButton = make("button", "Back to conversations");
  closeButton.type = "button";
  closeButton.className = "quiet-button";
  closeButton.dataset.libraryAction = "close";

  header.append(heading, note, closeButton);

  const controlsCard = createUiCard(doc, {
    title: "Retrieval & Context Query",
    description: "Search indexed conversations, documents or multi-term weighted retrieval.",
    className: "search-controls-card",
  });

  const form = make("form");
  form.className = "knowledge-library-form search-query-form";

  const primaryQueryRow = make("div");
  primaryQueryRow.className = "search-primary-row";

  const label = make("label", "Search phrase");
  label.htmlFor = "library-query";

  const queryInput = make("input");
  queryInput.type = "search";
  queryInput.id = "library-query";
  queryInput.maxLength = 256;
  queryInput.autocomplete = "off";
  queryInput.placeholder = "Enter search phrase or multi-query terms";

  const submitButton = make("button", "Search");
  submitButton.type = "submit";
  submitButton.className = "primary-button";

  const toggleMultiQueryBtn = make("button", "Multi-Query & Policies");
  toggleMultiQueryBtn.type = "button";
  toggleMultiQueryBtn.className = "quiet-button";
  toggleMultiQueryBtn.dataset.action = "toggle-multi-query";
  toggleMultiQueryBtn.setAttribute("aria-expanded", "false");

  primaryQueryRow.append(label, queryInput, submitButton, toggleMultiQueryBtn);
  form.append(primaryQueryRow);

  const multiQueryPanel = make("div");
  multiQueryPanel.className = "search-multi-query-panel";
  multiQueryPanel.hidden = true;

  const termsContainer = make("div");
  termsContainer.className = "search-terms-container";

  const addTermBtn = make("button", "+ Add Query Term");
  addTermBtn.type = "button";
  addTermBtn.className = "quiet-button";
  addTermBtn.dataset.action = "add-query-term";

  const policiesGrid = make("div");
  policiesGrid.className = "search-policies-grid";

  // Preset selector
  const presetLabel = make("label", "Preset: ");
  const presetSelect = make("select");
  presetSelect.className = "search-select";
  for (const preset of ["fast", "balanced", "source_first", "challenge", "historical"] as const) {
    const opt = make("option", preset.charAt(0).toUpperCase() + preset.slice(1));
    opt.value = preset;
    if (preset === selectedPreset) opt.selected = true;
    presetSelect.append(opt);
  }
  presetLabel.append(presetSelect);

  // Filters: Provider, Role, Kind
  const providerLabel = make("label", "Provider: ");
  const providerSelect = make("select");
  providerSelect.className = "search-select";
  for (const [val, text] of [["", "All Providers"], ["chatgpt", "ChatGPT"], ["claude", "Claude"], ["gemini", "Gemini"]]) {
    const opt = make("option", text);
    opt.value = val;
    providerSelect.append(opt);
  }
  providerLabel.append(providerSelect);

  const roleLabel = make("label", "Role: ");
  const roleSelect = make("select");
  roleSelect.className = "search-select";
  for (const [val, text] of [["", "All Roles"], ["user", "User"], ["assistant", "Assistant"], ["system", "System"]]) {
    const opt = make("option", text);
    opt.value = val;
    roleSelect.append(opt);
  }
  roleLabel.append(roleSelect);

  const kindLabel = make("label", "Kind: ");
  const kindSelect = make("select");
  kindSelect.className = "search-select";
  for (const [val, text] of [["", "All Kinds"], ["body", "Body text"], ["title", "Title"], ["code", "Code snippet"]]) {
    const opt = make("option", text);
    opt.value = val;
    kindSelect.append(opt);
  }
  kindLabel.append(kindSelect);

  // As-of date
  const asOfLabel = make("label", "As-of Date: ");
  const asOfInput = make("input");
  asOfInput.type = "date";
  asOfInput.className = "search-input-date";
  asOfLabel.append(asOfInput);

  // Budgets
  const budgetLabel = make("label", "Token Budget: ");
  const budgetInput = make("input");
  budgetInput.type = "number";
  budgetInput.min = "500";
  budgetInput.max = "32000";
  budgetInput.step = "500";
  budgetInput.value = String(tokenBudget);
  budgetLabel.append(budgetInput);

  policiesGrid.append(presetLabel, providerLabel, roleLabel, kindLabel, asOfLabel, budgetLabel);
  multiQueryPanel.append(termsContainer, addTermBtn, policiesGrid);
  form.append(multiQueryPanel);
  controlsCard.body.append(form);

  // Filters navigation
  const tabs = make("nav");
  tabs.className = "knowledge-library-filters search-tabs";
  tabs.setAttribute("aria-label", "Source filter");
  const filterDefs: Array<[Filter, string]> = [
    ["all", "All"],
    ["conversations", "Conversations"],
    ["files", "Files"],
    ["retrieval", "Structured Retrieval"],
  ];
  for (const [id, caption] of filterDefs) {
    const b = make("button", caption);
    b.type = "button";
    b.dataset.libraryFilter = id;
    b.setAttribute("aria-pressed", String(id === filter));
    tabs.append(b);
  }

  // Status & action bar
  const statusBar = make("div");
  statusBar.className = "search-status-bar";

  const status = make("p");
  status.className = "list-note search-status-text";
  status.setAttribute("role", "status");

  const actionButtons = make("div");
  actionButtons.className = "search-action-buttons";

  const buildContextBtn = make("button", "Build Context Pack");
  buildContextBtn.type = "button";
  buildContextBtn.className = "quiet-button";
  buildContextBtn.dataset.action = "build-context";
  buildContextBtn.disabled = true;

  const inspectTraceBtn = make("button", "Inspect Technical Trace");
  inspectTraceBtn.type = "button";
  inspectTraceBtn.className = "quiet-button";
  inspectTraceBtn.dataset.action = "inspect-trace";
  inspectTraceBtn.disabled = true;

  actionButtons.append(buildContextBtn, inspectTraceBtn);
  statusBar.append(status, actionButtons);

  // Results area
  const resultsContainer = make("div");
  resultsContainer.className = "search-results-area";

  const results = make("div");
  results.className = "knowledge-library-results search-results-list";
  results.setAttribute("aria-label", "Search results");

  const paginationNav = make("nav");
  paginationNav.className = "search-pagination-nav";
  paginationNav.hidden = true;

  const prevPageBtn = make("button", "Previous");
  prevPageBtn.type = "button";
  prevPageBtn.className = "quiet-button";
  prevPageBtn.dataset.action = "prev-page";

  const pageIndicator = make("span", "Page 1");
  pageIndicator.className = "search-page-indicator";

  const nextPageBtn = make("button", "Next");
  nextPageBtn.type = "button";
  nextPageBtn.className = "quiet-button";
  nextPageBtn.dataset.action = "next-page";

  paginationNav.append(prevPageBtn, pageIndicator, nextPageBtn);
  resultsContainer.append(results, paginationNav);

  // Detail / Preview panel
  const preview = make("section");
  preview.className = "knowledge-library-preview search-preview-panel";
  preview.setAttribute("aria-label", "Source and version preview");
  preview.append(make("p", "Choose a match to preview its provenance."));

  // Context Pack drawer / modal
  const contextDrawer = createUiDrawer(doc, {
    id: "search-context-drawer",
    title: "Retrieval Context Pack",
    description: "Verified assembled citations and excerpt budget pack.",
    placement: "right",
  });
  const contextDrawerBody = contextDrawer.querySelector<HTMLElement>(".ui-dialog-body")!;
  const contextPackContent = make("div");
  contextPackContent.className = "search-context-pack-content";
  const copyContextBtn = make("button", "Copy Context Pack");
  copyContextBtn.type = "button";
  copyContextBtn.className = "primary-button";
  copyContextBtn.dataset.action = "copy-context";
  contextDrawerBody.append(copyContextBtn, contextPackContent);

  // Technical Trace drawer / modal
  const traceDrawer = createUiDrawer(doc, {
    id: "search-trace-drawer",
    title: "Technical Trace & Diagnostic Inspector",
    description: "Inspect SQLite FTS5 BM25 breakdown, candidate evaluation and latency.",
    placement: "right",
  });
  const traceDrawerBody = traceDrawer.querySelector<HTMLElement>(".ui-dialog-body")!;
  const traceContent = make("div");
  traceContent.className = "search-trace-content";
  traceDrawerBody.append(traceContent);

  shell.append(header, controlsCard.element, tabs, statusBar, resultsContainer, preview, contextDrawer, traceDrawer);
  view.replaceChildren(shell);
  view.setAttribute("aria-busy", "false");

  function renderTermsList() {
    termsContainer.replaceChildren();
    for (let i = 0; i < queryTerms.length; i++) {
      const row = make("div");
      row.className = "search-term-row";
      const termInput = make("input");
      termInput.type = "text";
      termInput.value = queryTerms[i].query;
      termInput.placeholder = `Query term ${i + 1}`;
      termInput.dataset.termIndex = String(i);
      termInput.addEventListener("input", () => {
        queryTerms[i].query = termInput.value;
        if (i === 0) queryInput.value = termInput.value;
      });

      const weightInput = make("input");
      weightInput.type = "number";
      weightInput.min = "0.1";
      weightInput.max = "5.0";
      weightInput.step = "0.1";
      weightInput.value = String(queryTerms[i].weight);
      weightInput.dataset.weightIndex = String(i);
      weightInput.title = "Weight";
      weightInput.addEventListener("input", () => {
        const val = parseFloat(weightInput.value);
        if (Number.isFinite(val) && val > 0) queryTerms[i].weight = val;
      });

      row.append(termInput, weightInput);

      if (queryTerms.length > 1) {
        const removeBtn = make("button", "×");
        removeBtn.type = "button";
        removeBtn.className = "icon-button";
        removeBtn.title = "Remove term";
        removeBtn.addEventListener("click", () => {
          queryTerms.splice(i, 1);
          renderTermsList();
        });
        row.append(removeBtn);
      }
      termsContainer.append(row);
    }
  }

  function getCombinedMatchesCount(): number {
    let count = 0;
    if (filter === "all" || filter === "conversations") count += archiveHits.length;
    if (filter === "all" || filter === "files") count += fileHits.length;
    if (filter === "all" || filter === "retrieval") count += retrievalHits.length;
    return count;
  }

  function renderResults() {
    results.replaceChildren();

    type CombinedItem =
      | { type: "archive"; hit: ArchiveSearchHit; index: number }
      | { type: "file"; hit: DocumentSearchHit; index: number }
      | { type: "retrieval"; hit: RetrievalHit; index: number };

    const items: CombinedItem[] = [];

    if (filter === "all" || filter === "conversations") {
      archiveHits.forEach((hit, index) => items.push({ type: "archive", hit, index }));
    }
    if (filter === "all" || filter === "files") {
      fileHits.forEach((hit, index) => items.push({ type: "file", hit, index }));
    }
    if (filter === "all" || filter === "retrieval") {
      retrievalHits.forEach((hit, index) => items.push({ type: "retrieval", hit, index }));
    }

    const totalCount = items.length;
    const totalPages = Math.max(1, Math.ceil(totalCount / pageSize));
    if (currentPage > totalPages) currentPage = totalPages;
    if (currentPage < 1) currentPage = 1;

    const startIndex = (currentPage - 1) * pageSize;
    const pageItems = items.slice(startIndex, startIndex + pageSize);

    for (const item of pageItems) {
      if (item.type === "archive") {
        const hit = item.hit;
        const button = make("button");
        button.type = "button";
        button.className = "knowledge-library-hit search-hit-card";
        button.dataset.libraryArchive = String(item.index);

        const titleRow = make("div");
        titleRow.className = "search-hit-title-row";
        titleRow.append(
          make("strong", hit.title || "Untitled conversation"),
          createUiStatus(doc, "Conversation", "complete"),
        );

        const meta = make("small", `${hit.role} · ${hit.kind} · score: ${hit.score.toFixed(2)}`);
        meta.className = "search-hit-meta";

        const snippet = make("span", hit.snippet);
        snippet.className = "search-hit-snippet";

        button.append(titleRow, meta, snippet);
        results.append(button);
      } else if (item.type === "file") {
        const hit = item.hit;
        const button = make("button");
        button.type = "button";
        button.className = "knowledge-library-hit search-hit-card";
        button.dataset.libraryFile = String(item.index);

        const titleRow = make("div");
        titleRow.className = "search-hit-title-row";
        titleRow.append(make("strong", hit.uri), createUiStatus(doc, "Document", "neutral"));

        const meta = make("small", `Section: ${hit.section} · version ${hit.version_id}`);
        meta.className = "search-hit-meta";

        const snippet = make("span", hit.snippet);
        snippet.className = "search-hit-snippet";

        button.append(titleRow, meta, snippet);
        results.append(button);
      } else if (item.type === "retrieval") {
        const hit = item.hit;
        const button = make("button");
        button.type = "button";
        button.className = "knowledge-library-hit search-hit-card retrieval-hit-card";
        button.dataset.libraryRetrieval = String(item.index);

        const titleRow = make("div");
        titleRow.className = "search-hit-title-row";
        titleRow.append(
          make("strong", hit.title || hit.conversation_id),
          createUiStatus(doc, `${hit.provider} · ${hit.role}`, "complete"),
        );

        const queryTags = hit.matched_queries.map((q) => `Q${q + 1}`).join(", ");
        const meta = make(
          "small",
          `Score: ${hit.score.toFixed(2)}${queryTags ? ` · matches ${queryTags}` : ""} · ${hit.captured_at || "archived"}`,
        );
        meta.className = "search-hit-meta";

        const snippet = make("span", hit.snippet);
        snippet.className = "search-hit-snippet";

        button.append(titleRow, meta, snippet);
        results.append(button);
      }
    }

    if (totalCount === 0) {
      results.append(make("p", "No visible matches. Try a different phrase or source filter."));
      paginationNav.hidden = true;
    } else {
      paginationNav.hidden = totalPages <= 1;
      pageIndicator.textContent = `Page ${currentPage} of ${totalPages} (${totalCount} matches)`;
      prevPageBtn.disabled = currentPage <= 1;
      nextPageBtn.disabled = currentPage >= totalPages;
    }
  }

  async function checkSecurity(): Promise<SecuritySnapshot> {
    const sec = await nativeInvoke<SecuritySnapshot>("get_security_status").catch(() => null);
    if (!alive || !sec || sec.locked !== false || !Number.isSafeInteger(sec.epoch)
      || (securityEpoch !== null && sec.epoch !== securityEpoch)) {
      clearEvidence(); surfaceStatus = "locked";
      results.replaceChildren(); preview.replaceChildren(); contextPackContent.replaceChildren(); traceContent.replaceChildren();
      contextDrawer.removeAttribute("open"); traceDrawer.removeAttribute("open");
      buildContextBtn.disabled = true; inspectTraceBtn.disabled = true;
      throw new Error("MP_SECURITY_LOCKED");
    }
    securityEpoch = sec.epoch;
    return sec;
  }

  async function search() {
    const primaryQuery = queryInput.value.trim();
    if (!primaryQuery || primaryQuery.length > 256) {
      status.textContent = "Enter a search phrase (1–256 characters).";
      return;
    }
    queryTerms[0].query = primaryQuery;

    clearEvidence();
    contextPackContent.replaceChildren(); traceContent.replaceChildren();
    buildContextBtn.disabled = true; inspectTraceBtn.disabled = true;
    ++previewEpoch;
    preview.replaceChildren(make("p", "Choose a match to preview its provenance."));
    const version = ++epoch;

    submitButton.disabled = true;
    status.textContent = "Searching currently accessible indexes…";
    surfaceStatus = "loading";
    results.replaceChildren();

    try {
      await checkSecurity();

      const archivesPromise = Promise.allSettled([
        invoke<ArchiveSearchHit[]>("search_archive_index", { query: primaryQuery, includeHistory: true, limit: 40 }),
      ]).then(([res]) => res);

      const filesPromise = Promise.allSettled([
        invoke<DocumentSearchHit[]>("search_document_index", { query: primaryQuery, limit: 40 }),
      ]).then(([res]) => res);

      let retrievalPromise: Promise<PromiseSettledResult<RetrievalResponse> | null> = Promise.resolve(null);
      // Run structured retrieval if multi-query mode or retrieval filter selected
      if (multiQueryMode || filter === "retrieval") {
        const validTerms = queryTerms.filter((t) => t.query.trim().length > 0);
        const retrievalReq: {
          queries: QueryTerm[];
          filters: RetrievalFilters;
          limit: number;
        } = {
          queries: validTerms.length > 0 ? validTerms : [{ query: primaryQuery, weight: 1.0 }],
          filters: {
            providers: selectedProvider ? [selectedProvider] : undefined,
            roles: selectedRole ? [selectedRole] : undefined,
            kinds: selectedKind ? [selectedKind] : undefined,
            captured_to: asOfDate ? new Date(asOfDate + "T23:59:59.999Z").toISOString() : undefined,
          },
          limit: 40,
        };
        retrievalPromise = Promise.allSettled([
          invoke<RetrievalResponse>("search_retrieval", { request: retrievalReq }),
        ]).then(([res]) => res);
      }

      const [archives, files, retrieval] = await Promise.all([
        archivesPromise,
        filesPromise,
        retrievalPromise,
      ]);

      if (!alive || version !== epoch) return;
      await checkSecurity();
      if (!alive || version !== epoch) return;
      archiveHits = archives.status === "fulfilled" && Array.isArray(archives.value) ? archives.value.slice(0, 40) : [];
      fileHits = files.status === "fulfilled" && Array.isArray(files.value) ? files.value.slice(0, 40) : [];
      retrievalHits = retrieval && retrieval.status === "fulfilled" && retrieval.value?.hits ? retrieval.value.hits : [];

      const failed: string[] = [];
      if (archives.status !== "fulfilled") failed.push("conversation index");
      if (files.status !== "fulfilled") failed.push("document index");
      if (retrieval && retrieval.status !== "fulfilled") failed.push("structured retrieval");

      currentPage = 1;
      surfaceStatus = "ready";

      const totalMatches = archiveHits.length + fileHits.length + retrievalHits.length;
      status.textContent =
        `${archiveHits.length} conversation matches · ${fileHits.length} file matches` +
        (retrievalHits.length ? ` · ${retrievalHits.length} retrieval matches` : "") +
        " (bounded results)." +
        (failed.length ? " Unavailable: " + failed.join(", ") + ". Results may be incomplete." : "");

      buildContextBtn.disabled = totalMatches === 0;
      inspectTraceBtn.disabled = totalMatches === 0;

      submitButton.disabled = false;
      renderResults();
    } catch (err: any) {
      if (!alive || version !== epoch) return;
      submitButton.disabled = false;
      if (err?.message?.includes("LOCKED")) {
        surfaceStatus = "locked";
        results.replaceChildren(
          createUiStatePanel(doc, {
            state: "locked",
            title: "Mind Place Locked",
            detail: "The archive is locked. Unlock to search.",
          }),
        );
        status.textContent = "Mind Place is locked.";
      } else {
        surfaceStatus = "error";
        status.textContent = "Search error: " + (err?.message || "Unknown error");
      }
    }
  }

  async function previewArchive(hit: ArchiveSearchHit) {
    const version = ++previewEpoch;
    preview.replaceChildren(make("p", "Checking selected snapshot…"));
    try {
      if (!hit.archive_id || !hit.snapshot_id) throw new Error("Missing archive identity");
      const result = await invoke<Conversation>("read_conversation", {
        id: hit.archive_id,
        snapshotId: hit.snapshot_id,
      });
      if (!alive || version !== previewEpoch) return;
      if (result.summary.id !== hit.archive_id || (result.snapshot_id && result.snapshot_id !== hit.snapshot_id)) {
        throw new Error("Returned source/version differs from selected index result");
      }
      const titleHit = hit.kind === "title" && hit.message_id === "@title";
      const matching = titleHit
        ? undefined
        : result.capture.messages.find((message) => message.id === hit.message_id);
      if (!titleHit && !matching) throw new Error("Selected source message not available in this snapshot");
      const sourceText = titleHit ? result.capture.title : matching!.text;
      preview.replaceChildren(
        make("h2", result.summary.title || "Conversation source"),
        make("p", "Archive ID: " + hit.archive_id),
        make("p", "Snapshot: " + hit.snapshot_id + (titleHit ? " · title" : " · message " + hit.message_id)),
        make("p", "Provider: " + result.summary.provider + " · captured " + result.capture.captured_at),
        make("pre", sourceText.slice(0, 1800) + (sourceText.length > 1800 ? "…" : "")),
      );
      if (options.onSelectConversation) {
        const open = createUiButton(doc, "Open full saved revision");
        open.dataset.action = "open-search-conversation";
        open.addEventListener("click", () => { void checkSecurity().then(() => {
          if (alive && version === previewEpoch) options.onSelectConversation?.(hit.archive_id, hit.snapshot_id);
        }).catch(() => {}); });
        preview.append(open);
      }
    } catch {
      if (alive && version === previewEpoch) {
        preview.replaceChildren(
          make("p", "Source preview unavailable or no longer authorized. Refresh the search."),
        );
      }
    }
  }

  async function previewFile(hit: DocumentSearchHit) {
    const version = ++previewEpoch;
    try {
    const exact = await invoke<DocumentPreview>("preview_document_source", { uri: hit.uri, versionId: hit.version_id });
    if (!alive || version !== previewEpoch) return;
    if (exact.uri !== hit.uri || exact.version_id !== hit.version_id) throw new Error("Source changed");
    preview.replaceChildren(
      make("h2", "Indexed document excerpt"),
      make("p", "Source: " + hit.uri),
      make("p", "Version: " + hit.version_id + " · section: " + hit.section),
      make("p", "Extractor: " + hit.extractor_revision),
      make("pre", exact.text),
      make(
        "p",
        hit.warnings.length
          ? "Warnings: " + hit.warnings.join("; ")
          : "Excerpt only; indexing and live root permission checks still apply.",
      ),
    );

    } catch { if (alive && version === previewEpoch) preview.replaceChildren(make("p", "Source preview unavailable or no longer authorized.")); }
  }

  async function previewRetrieval(hit: RetrievalHit) {
    const version = ++previewEpoch;
    try { await checkSecurity(); } catch { return; }
    if (!alive || version !== previewEpoch) return;
    preview.replaceChildren(
      make("h2", hit.title || "Retrieval Hit Provenance"),
      make("p", `Source: ${hit.source_id} · Snapshot: ${hit.snapshot_id}`),
      make("p", `Provider: ${hit.provider} · Role: ${hit.role} · Kind: ${hit.kind}`),
      make("p", `Captured at: ${hit.captured_at} · Score: ${hit.score.toFixed(4)}`),
      make("pre", hit.snippet),
    );
  }

  async function buildContext() {
    if (selectedPreset === "historical" && !asOfDate) { status.textContent = "Select an as-of date for Historical context."; return; }
    const version = epoch;
    buildContextBtn.disabled = true;
    status.textContent = "Assembling context pack…";
    const primaryQuery = queryInput.value.trim();
    const validTerms = queryTerms.filter((t) => t.query.trim().length > 0);
    const req: ContextRequest = {
      queries: validTerms.length > 0 ? validTerms : [{ query: primaryQuery, weight: 1.0 }],
      filters: {
        providers: selectedProvider ? [selectedProvider] : undefined,
        roles: selectedRole ? [selectedRole] : undefined,
        kinds: selectedKind ? [selectedKind] : undefined,
        captured_to: asOfDate ? new Date(asOfDate + "T23:59:59.999Z").toISOString() : undefined,
      },
      preset: asOfDate ? "historical" : selectedPreset,
      as_of: asOfDate ? new Date(asOfDate + "T23:59:59.999Z").toISOString() : null,
      max_sources: maxSources,
      max_excerpts: maxExcerpts,
      max_per_source: maxPerSource,
      budget: { max_estimated_tokens: tokenBudget },
    };

    try {
      await checkSecurity();
      const pack = await invoke<ContextPack>("build_retrieval_context", { request: req });
      if (!alive || version !== epoch) return;
      lastContextPack = pack;

      contextPackContent.replaceChildren();

      const summaryCard = createUiCard(doc, {
        title: "Context Summary",
        description: `${pack.groups.length} source groups · estimated ${pack.estimated_tokens} tokens${pack.truncated ? " (budget truncated)" : ""}`,
      });
      contextPackContent.append(summaryCard.element);

      for (const group of pack.groups) {
        const groupCard = createUiCard(doc, {
          title: group.title || group.conversation_id,
          description: `Provider: ${group.source_id} · Snapshot: ${group.snapshot_id} · Captured: ${group.captured_at}`,
        });
        for (const excerpt of group.excerpts) {
          const excerptEl = make("div");
          excerptEl.className = "search-context-excerpt";
          const meta = make("small", `Role: ${excerpt.role} · bytes ${excerpt.byte_start}–${excerpt.byte_end} · score: ${excerpt.score.toFixed(2)}`);
          const text = make("pre", excerpt.text);
          excerptEl.append(meta, text);
          groupCard.body.append(excerptEl);
        }
        contextPackContent.append(groupCard.element);
      }

      status.textContent = `Context pack built: ${pack.groups.length} sources, ~${pack.estimated_tokens} tokens.`;
      if (typeof contextDrawer.showModal === "function") {
        try {
          contextDrawer.showModal();
        } catch {
          contextDrawer.setAttribute("open", "");
        }
      } else {
        contextDrawer.setAttribute("open", "");
      }
    } catch (err: any) {
      if (!alive) return;
      status.textContent = "Context build failed: " + (err?.message || "Error");
    } finally {
      if (alive) buildContextBtn.disabled = false;
    }
  }

  async function inspectTrace() {
    const version = epoch;
    inspectTraceBtn.disabled = true;
    status.textContent = "Running technical inspection…";
    const primaryQuery = queryInput.value.trim();
    const validTerms = queryTerms.filter((t) => t.query.trim().length > 0);
    const req = {
      queries: validTerms.length > 0 ? validTerms : [{ query: primaryQuery, weight: 1.0 }],
      filters: {
        providers: selectedProvider ? [selectedProvider] : undefined,
        roles: selectedRole ? [selectedRole] : undefined,
        kinds: selectedKind ? [selectedKind] : undefined,
        captured_to: asOfDate ? new Date(asOfDate + "T23:59:59.999Z").toISOString() : undefined,
      },
      limit: 10,
    };

    try {
      await checkSecurity();
      const inspection = await invoke<SearchInspection>("inspect_retrieval", { request: req });
      if (!alive || version !== epoch) return;
      lastTrace = inspection.trace;
      lastScoreContributions = inspection.result.hits[0]?.score_contributions ?? [];

      traceContent.replaceChildren();

      const t = inspection.trace;
      const metricsCard = createUiCard(doc, {
        title: "Trace Metrics",
        description: `Operation: ${t.operation} · Index Generation: ${t.index_generation}`,
      });
      const metricsList = [
        ["Elapsed time", `${t.elapsed_us} µs`],
        ["Index Generation", String(t.index_generation)],
        ["Candidates evaluated", String(t.candidate_count)],
        ["Results returned", String(t.result_count)],
        ["Sources examined", String(t.source_count)],
        ["Estimated tokens", String(t.estimated_tokens)],
        ["Truncated", t.truncated ? "Yes" : "No"],
      ];
      for (const [label, val] of metricsList) {
        const row = make("div");
        row.className = "search-trace-metric-row";
        row.append(make("span", label + ": "), make("strong", val));
        metricsCard.body.append(row);
      }
      traceContent.append(metricsCard.element);

      if (lastScoreContributions.length > 0) {
        const scoreTable = createUiTable(doc, {
          caption: "Per-query BM25 & weighted relevance contributions (top hit)",
          columns: ["Query Index", "Weight", "BM25", "Lexical Rel", "Exact Bonus", "Weighted Score"],
          rows: lastScoreContributions.map((sc) => [
            String(sc.query_index),
            sc.weight.toFixed(2),
            sc.bm25.toFixed(4),
            sc.lexical_relevance.toFixed(4),
            sc.exact_bonus.toFixed(4),
            sc.weighted_score.toFixed(4),
          ]),
        });
        traceContent.append(scoreTable);
      }

      status.textContent = `Inspection trace complete (${t.elapsed_us} µs).`;
      if (typeof traceDrawer.showModal === "function") {
        try {
          traceDrawer.showModal();
        } catch {
          traceDrawer.setAttribute("open", "");
        }
      } else {
        traceDrawer.setAttribute("open", "");
      }
    } catch (err: any) {
      if (!alive) return;
      status.textContent = "Inspection failed: " + (err?.message || "Error");
    } finally {
      if (alive) inspectTraceBtn.disabled = false;
    }
  }

  async function copyContextPack() {
    if (!lastContextPack || !options.clipboard) return;
    try {
      await checkSecurity();
      if (!alive || !lastContextPack) return;
      const text = JSON.stringify(lastContextPack, null, 2);
      await options.clipboard.writeText(text);
      status.textContent = "Context pack copied to clipboard.";
    } catch {
      status.textContent = "Failed to copy context pack.";
    }
  }

  const submitHandler = (event: Event) => {
    event.preventDefault();
    if (alive) void search();
  };

  const clickHandler = (event: Event) => {
    const target = event.target;
    if (!alive || !(target instanceof doc.defaultView!.Element)) return;

    if (target.closest<HTMLElement>("[data-library-action='close']")) {
      options.onClose?.();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='toggle-multi-query']")) {
      multiQueryMode = !multiQueryMode;
      multiQueryPanel.hidden = !multiQueryMode;
      toggleMultiQueryBtn.setAttribute("aria-expanded", String(multiQueryMode));
      toggleMultiQueryBtn.textContent = multiQueryMode ? "Simple Search" : "Multi-Query & Policies";
      if (multiQueryMode) renderTermsList();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='add-query-term']")) {
      queryTerms.push({ query: "", weight: 1.0 });
      renderTermsList();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='build-context']")) {
      void buildContext();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='inspect-trace']")) {
      void inspectTrace();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='copy-context']")) {
      void copyContextPack();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='prev-page']")) {
      if (currentPage > 1) {
        currentPage--;
        renderResults();
      }
      return;
    }
    if (target.closest<HTMLElement>("[data-action='next-page']")) {
      currentPage++;
      renderResults();
      return;
    }
    if (target.closest<HTMLElement>("[data-action='close-ui-dialog']")) {
      const btn = target.closest<HTMLButtonElement>("[data-action='close-ui-dialog']")!;
      const dialogId = btn.dataset.dialogId;
      if (dialogId === "search-context-drawer") {
        if (typeof contextDrawer.close === "function") contextDrawer.close();
        else contextDrawer.removeAttribute("open");
      } else if (dialogId === "search-trace-drawer") {
        if (typeof traceDrawer.close === "function") traceDrawer.close();
        else traceDrawer.removeAttribute("open");
      }
      return;
    }

    const tab = target.closest<HTMLButtonElement>("[data-library-filter]");
    if (tab) {
      filter = tab.dataset.libraryFilter as Filter;
      for (const button of tabs.querySelectorAll<HTMLButtonElement>("[data-library-filter]")) {
        button.setAttribute("aria-pressed", String(button === tab));
      }
      currentPage = 1;
      renderResults();
      return;
    }

    const archive = target.closest<HTMLElement>("[data-library-archive]");
    if (archive) {
      const hit = archiveHits[Number(archive.dataset.libraryArchive)];
      if (hit) void previewArchive(hit);
      return;
    }

    const file = target.closest<HTMLElement>("[data-library-file]");
    if (file) {
      const hit = fileHits[Number(file.dataset.libraryFile)];
      if (hit) void previewFile(hit);
      return;
    }

    const ret = target.closest<HTMLElement>("[data-library-retrieval]");
    if (ret) {
      const hit = retrievalHits[Number(ret.dataset.libraryRetrieval)];
      if (hit) void previewRetrieval(hit);
    }
  };

  presetSelect.addEventListener("change", () => {
    selectedPreset = presetSelect.value as ContextPreset;
  });
  providerSelect.addEventListener("change", () => {
    selectedProvider = providerSelect.value;
  });
  roleSelect.addEventListener("change", () => {
    selectedRole = roleSelect.value;
  });
  kindSelect.addEventListener("change", () => {
    selectedKind = kindSelect.value;
  });
  asOfInput.addEventListener("change", () => {
    asOfDate = asOfInput.value;
  });
  budgetInput.addEventListener("change", () => {
    const val = parseInt(budgetInput.value, 10);
    if (Number.isSafeInteger(val) && val >= 500) tokenBudget = val;
  });

  form.addEventListener("submit", submitHandler);
  shell.addEventListener("click", clickHandler);

  return {
    state: () => ({
      status: surfaceStatus,
      data: { archiveHits, fileHits, retrievalHits, lastContextPack, lastTrace },
      error: null,
      permission: "granted",
      vault: surfaceStatus === "locked" ? "locked" : securityEpoch === null ? "not-applicable" : "unlocked",
    }),
    refresh: async () => {
      if (queryInput.value.trim()) await search();
    },
    destroy: () => {
      alive = false;
      clearEvidence();
      epoch++;
      previewEpoch++;
      form.removeEventListener("submit", submitHandler);
      shell.removeEventListener("click", clickHandler);
      shell.replaceChildren();
      view.replaceChildren();
    },
  };
}
