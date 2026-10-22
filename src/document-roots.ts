import type { Invoke } from "./reader.js";

const REVOKE_CONFIRM_WINDOW_MS = 15_000;

export interface DocumentSearchHit {
  uri: string;
  version_id: string;
  section: string;
  snippet: string;
  score: number;
  extractor_revision: string;
  warnings: string[];
}

export interface DocumentIndexReport {
  root_id: string;
  scanned_files: number;
  indexed_files: number;
  skipped_files: number;
  indexed_chunks: number;
  generation: number;
  auto_refresh_active: boolean;
}

export interface DocumentRoot {
  id: string;
  canonical_path: string;
  approval_state: "pending" | "approved" | "revoked";
  virtual_prefix: string;
}

/** Unlock-scoped OWNER setup, never mounted in the MCP/public read surface. */
export function mountDocumentRootSetup(parent: HTMLElement, invoke: Invoke): void {
  const doc = parent.ownerDocument;
  const section = doc.createElement("section");
  section.className = "document-root-setup";
  const heading = doc.createElement("h3");
  heading.className = "setup-section-title";
  heading.textContent = "Personal document folders — owner permissions";
  const explanation = doc.createElement("p");
  explanation.textContent =
    "Staging only records a pending identity. Approve explicitly, then select Index folder to discover files and begin a 5-minute refresh schedule until paused, locked or revoked. Revocation is permanent.";
  const form = doc.createElement("form");
  form.className = "document-root-form";
  const label = doc.createElement("label");
  label.textContent = "Existing folder (absolute path)";
  const input = doc.createElement("input");
  input.type = "text";
  input.name = "document-root-path";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.maxLength = 4096;
  input.placeholder = "C:\\Users\\…\\Documents";
  label.append(input);
  const stage = doc.createElement("button");
  stage.type = "submit";
  stage.className = "quiet-button";
  stage.textContent = "Stage folder";
  form.append(label, stage);
  const status = doc.createElement("p");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const list = doc.createElement("ul");
  list.className = "document-root-list";
  const searchForm = doc.createElement("form");
  searchForm.className = "document-search-form";
  const searchLabel = doc.createElement("label");
  searchLabel.textContent = "Search approved, indexed documents";
  const searchInput = doc.createElement("input");
  searchInput.type = "search";
  searchInput.maxLength = 256;
  searchInput.autocomplete = "off";
  searchInput.placeholder = "Exact phrase…";
  searchLabel.append(searchInput);
  const searchButton = doc.createElement("button");
  searchButton.type = "submit";
  searchButton.className = "quiet-button";
  searchButton.textContent = "Search documents";
  searchForm.append(searchLabel, searchButton);
  const searchStatus = doc.createElement("p");
  searchStatus.setAttribute("role", "status");
  searchStatus.setAttribute("aria-live", "polite");
  const results = doc.createElement("ul");
  results.className = "document-search-results";
  results.setAttribute("aria-label", "Document search results");
  let searchEpoch = 0;
  section.append(heading, explanation, form, status, list, searchForm, searchStatus, results);
  parent.append(section);

  const refresh = async (): Promise<void> => {
    try {
      const roots = await invoke<DocumentRoot[]>("list_document_roots");
      if (!section.isConnected) return;
      list.replaceChildren();
      if (!roots.length) {
        const item = doc.createElement("li");
        item.textContent = "No document folders staged.";
        list.append(item);
      }
      for (const root of roots) {
        const item = doc.createElement("li");
        const name = doc.createElement("strong");
        name.textContent = root.canonical_path;
        const details = doc.createElement("small");
        details.textContent = ` ${root.approval_state.toUpperCase()} · ${root.virtual_prefix}`;
        item.append(name, details);
        if (root.approval_state === "pending") {
          const approve = doc.createElement("button");
          approve.type = "button";
          approve.className = "quiet-button";
          approve.dataset.action = "approve-root";
          approve.textContent = "Approve folder access";
          approve.addEventListener("click", () => {
            if (!section.isConnected) return;
            approve.disabled = true;
            status.textContent = "Checking folder identity and recording owner approval…";
            void invoke<DocumentRoot>("approve_document_root", { rootId: root.id })
              .then(async () => {
                if (!section.isConnected) return;
                status.textContent = "Approved. Select Index folder explicitly to discover files.";
                await refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Approval denied: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) approve.disabled = false; });
          });
          item.append(approve);
        }
        if (root.approval_state === "approved") {
          const index = doc.createElement("button");
          index.type = "button";
          index.className = "quiet-button";
          index.dataset.action = "index-root";
          index.textContent = "Index folder";
          index.addEventListener("click", () => {
            index.disabled = true;
            status.textContent = "Scanning approved files and building a disposable index…";
            void invoke<DocumentIndexReport>("refresh_document_root_index", { rootId: root.id })
              .then((report) => {
                if (!section.isConnected) return;
                status.textContent = `Indexed ${report.indexed_files} files / ${report.indexed_chunks} chunks (${report.skipped_files} skipped). Generation ${report.generation}. ${report.auto_refresh_active ? "Automatic 5-minute refresh enabled until Lock, Pause or Revoke." : "Automatic refresh capacity reached; manual indexing remains available."}`;
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Document indexing failed: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) index.disabled = false; });
          });
          item.append(index);
          const pause = doc.createElement("button");
          pause.type = "button";
          pause.className = "quiet-button";
          pause.dataset.action = "pause-root-refresh";
          pause.textContent = "Pause automatic refresh";
          pause.addEventListener("click", () => {
            pause.disabled = true;
            void invoke<void>("pause_document_root_refresh", { rootId: root.id })
              .then(() => {
                if (section.isConnected) status.textContent = "Periodic folder refresh paused. Manual indexing remains available.";
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Could not pause folder refresh: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) pause.disabled = false; });
          });
          item.append(pause);
        }
        if (root.approval_state !== "revoked") {
          const revoke = doc.createElement("button");
          revoke.type = "button";
          revoke.className = "quiet-button";
          revoke.dataset.action = "revoke-root";
          revoke.textContent = "Revoke";
          let armedAt: number | null = null;
          let resetTimer: ReturnType<typeof setTimeout> | undefined;
          const disarm = () => {
            armedAt = null;
            if (resetTimer !== undefined) clearTimeout(resetTimer);
            resetTimer = undefined;
            if (!revoke.disabled) revoke.textContent = "Revoke";
          };
          revoke.addEventListener("blur", () => {
            if (!revoke.disabled) disarm();
          });
          revoke.addEventListener("click", () => {
            if (!section.isConnected) return;
            const now = Date.now();
            if (armedAt === null || now - armedAt >= REVOKE_CONFIRM_WINDOW_MS) {
              disarm();
              armedAt = now;
              revoke.textContent = "Confirm permanent revoke (15s)";
              status.textContent = "Click again within 15 seconds to permanently revoke this folder.";
              resetTimer = setTimeout(() => {
                if (revoke.isConnected) disarm();
              }, REVOKE_CONFIRM_WINDOW_MS);
              return;
            }
            disarm();
            revoke.disabled = true;
            void invoke<void>("revoke_document_root", { rootId: root.id })
              .then(async () => {
                if (!section.isConnected) return;
                searchEpoch++;
                results.replaceChildren();
                searchStatus.textContent = "Results cleared after revocation.";
                status.textContent = "Document root permanently revoked.";
                await refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Revocation failed: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) revoke.disabled = false; });
          });
          item.append(revoke);
        }
        list.append(item);
      }
    } catch {
      if (section.isConnected) status.textContent = "Document registry unavailable while locked or offline.";
    }
  };

  searchForm.addEventListener("submit", (event) => {
    event.preventDefault();
    const query = searchInput.value.trim();
    if (!query || query.length > 256) {
      searchStatus.textContent = "Enter a document search phrase (up to 256 characters).";
      return;
    }
    const epoch = ++searchEpoch;
    searchButton.disabled = true;
    results.replaceChildren();
    searchStatus.textContent = "Checking indexed documents and live permissions…";
    void invoke<DocumentSearchHit[]>("search_document_index", { query, limit: 20 })
      .then((hits) => {
        if (!section.isConnected || epoch !== searchEpoch) return;
        searchStatus.textContent = `${hits.length} verified document matches. New/modified files require Index folder again.`;
        for (const hit of hits) {
          const item = doc.createElement("li");
          const title = doc.createElement("strong");
          title.textContent = hit.uri;
          const part = doc.createElement("small");
          part.textContent = ` · ${hit.section} · revision ${hit.version_id.slice(0, 20)}`;
          const excerpt = doc.createElement("p");
          excerpt.textContent = hit.snippet;
          item.append(title, part, excerpt);
          const caveats = doc.createElement("small");
          caveats.textContent = `Extractor ${hit.extractor_revision}${hit.warnings.length ? " · " + hit.warnings.join("; ") : ""}`;
          item.append(caveats);
          results.append(item);
        }
      })
      .catch((error: unknown) => {
        if (!section.isConnected || epoch !== searchEpoch) return;
        searchStatus.textContent = `Document search unavailable: ${String(error)}`;
      })
      .finally(() => { if (section.isConnected && epoch === searchEpoch) searchButton.disabled = false; });
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const path = input.value.trim();
    if (!path || path.length > 4096) {
      status.textContent = "Enter an existing absolute folder path.";
      return;
    }
    stage.disabled = true;
    status.textContent = "Staging pending document identity…";
    void invoke<DocumentRoot>("stage_document_root", { path })
      .then(async (root) => {
        if (!section.isConnected) return;
        input.value = "";
        status.textContent = `Staged: ${root.approval_state}. No files were read or indexed.`;
        await refresh();
      })
      .catch((error: unknown) => {
        if (section.isConnected) status.textContent = `Could not stage folder: ${String(error)}`;
      })
      .finally(() => { if (section.isConnected) stage.disabled = false; });
  });
  void refresh();
}
