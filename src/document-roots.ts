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

export interface DocumentPreview {
  uri: string;
  version_id: string;
  text: string;
  truncated: boolean;
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
  section.setAttribute("aria-label", "Document library and folder permissions");
  const heading = doc.createElement("h3");
  heading.className = "setup-section-title";
  heading.textContent = "Your document library";
  const explanation = doc.createElement("p");
  explanation.textContent =
    "Staging only records a pending identity. Approve explicitly, then select Index folder to discover files and begin a 5-minute refresh schedule until paused, locked or revoked. Revocation is permanent.";
  const steps = doc.createElement("ol");
  steps.className = "document-onboarding-steps";
  for (const [number, title, detail] of [
    ["01", "Stage", "Record an existing folder path. No content is read."],
    ["02", "Approve", "Grant access to that exact folder, explicitly."],
    ["03", "Index", "Scan permitted files when you choose to begin."],
  ]) {
    const item = doc.createElement("li");
    const count = doc.createElement("span");
    count.className = "document-step-number";
    count.textContent = number;
    const body = doc.createElement("span");
    const strong = doc.createElement("strong");
    strong.textContent = title;
    const detailLine = doc.createElement("small");
    detailLine.textContent = detail;
    body.append(strong, detailLine);
    item.append(count, body);
    steps.append(item);
  }
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
  input.id = "document-root-path";
  input.setAttribute("aria-describedby", "document-root-path-help");
  const pathHelp = doc.createElement("small");
  pathHelp.id = "document-root-path-help";
  pathHelp.className = "document-field-help";
  pathHelp.textContent = "Use an existing absolute folder path. Stage does not scan, approve, or index it.";
  label.append(input);
  const stage = doc.createElement("button");
  stage.type = "submit";
  stage.className = "quiet-button";
  stage.textContent = "Stage folder";
  form.append(label, pathHelp, stage);
  const status = doc.createElement("p");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const folderHeading = doc.createElement("h4");
  folderHeading.className = "document-section-heading";
  folderHeading.textContent = "Managed folders";
  const list = doc.createElement("ul");
  list.className = "document-root-list";
  const searchHeading = doc.createElement("h4");
  searchHeading.className = "document-section-heading";
  searchHeading.textContent = "Search indexed content";
  const searchNote = doc.createElement("p");
  searchNote.className = "document-field-help";
  searchNote.textContent = "Search covers approved indexed files, not every file on your device. Results show bounded, verified excerpts rather than full source previews.";
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
  let previewEpoch = 0;
  let revocationPending = false;
  const lastReports = new Map<string, { report: DocumentIndexReport; indexedAt: string; paused: boolean }>();
  section.append(heading, explanation, steps, form, status, folderHeading, list, searchHeading, searchNote, searchForm, searchStatus, results);
  parent.append(section);

  const refresh = async (): Promise<void> => {
    try {
      const roots = await invoke<DocumentRoot[]>("list_document_roots");
      if (!section.isConnected) return;
      list.replaceChildren();
      for (const known of [...lastReports.keys()]) {
        if (!roots.some((root) => root.id === known && root.approval_state === "approved")) lastReports.delete(known);
      }
      if (!roots.length) {
        const item = doc.createElement("li");
        item.className = "document-empty-state";
        item.textContent = "No document folders staged. Start with a folder path above.";
        list.append(item);
      }
      for (const root of roots) {
        const item = doc.createElement("li");
        item.className = "document-root-card";
        const top = doc.createElement("div");
        top.className = "document-root-card-header";
        const name = doc.createElement("strong");
        name.className = "document-root-path";
        name.textContent = root.canonical_path;
        const badge = doc.createElement("span");
        badge.className = `document-permission-badge is-${root.approval_state}`;
        badge.textContent = root.approval_state.toUpperCase();
        top.append(name, badge);
        const details = doc.createElement("small");
        details.className = "document-root-scope";
        details.textContent = `Authorized namespace: ${root.virtual_prefix}`;
        const note = doc.createElement("p");
        note.className = "document-root-state";
        const latest = lastReports.get(root.id);
        note.textContent = root.approval_state === "pending"
          ? "Pending consent · No files have been read."
          : root.approval_state === "revoked"
            ? "Permanently revoked · No future scans or search access."
            : latest
              ? `Last indexed in this session: ${latest.indexedAt} · ${latest.report.indexed_files} files · ${latest.report.skipped_files} skipped · ${latest.report.indexed_chunks} chunks · generation ${latest.report.generation} · refresh ${latest.paused ? "paused" : latest.report.auto_refresh_active ? "active" : "inactive"}.`
              : "Approved · Index status not available in this session. Index folder to scan. No automatic indexing on approval.";
        const actions = doc.createElement("div");
        actions.className = "document-root-actions";
        item.append(top, details, note, actions);
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
          actions.append(approve);
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
                lastReports.set(root.id, { report, indexedAt: new Date().toLocaleString(), paused: false });
                status.textContent = `Indexed ${report.indexed_files} files / ${report.indexed_chunks} chunks (${report.skipped_files} skipped). Generation ${report.generation}. ${report.auto_refresh_active ? "Automatic 5-minute refresh enabled until Lock, Pause or Revoke." : "Automatic refresh capacity reached; manual indexing remains available."}`;
                void refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Document indexing failed: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) index.disabled = false; });
          });
          actions.append(index);
          const pause = doc.createElement("button");
          pause.type = "button";
          pause.className = "quiet-button";
          pause.dataset.action = "pause-root-refresh";
          pause.textContent = "Pause automatic refresh";
          pause.addEventListener("click", () => {
            pause.disabled = true;
            void invoke<void>("pause_document_root_refresh", { rootId: root.id })
              .then(() => {
                if (section.isConnected) {
                  const current = lastReports.get(root.id);
                  if (current) {
                    current.paused = true;
                    void refresh();
                  }
                  status.textContent = "Periodic folder refresh paused. Manual indexing remains available.";
                }
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Could not pause folder refresh: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) pause.disabled = false; });
          });
          actions.append(pause);
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
            revocationPending = true;
            searchEpoch++;
            previewEpoch++;
            results.replaceChildren();
            searchButton.disabled = true;
            searchStatus.textContent = "Folder revocation is pending; previous search results were cleared.";
            void invoke<void>("revoke_document_root", { rootId: root.id })
              .then(async () => {
                if (!section.isConnected) return;
                searchEpoch++;
                results.replaceChildren();
                lastReports.delete(root.id);
                searchStatus.textContent = "Results cleared after revocation.";
                status.textContent = "Document root permanently revoked.";
                await refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Revocation failed: ${String(error)}`;
              })
              .finally(() => {
                revocationPending = false;
                if (section.isConnected) {
                  revoke.disabled = false;
                  searchButton.disabled = false;
                }
              });
          });
          revoke.classList.add("document-danger-button");
          actions.append(revoke);
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
    if (revocationPending) {
      searchStatus.textContent = "Search is paused until folder revocation finishes.";
      return;
    }
    if (!query || query.length > 256) {
      searchStatus.textContent = "Enter a document search phrase (up to 256 characters).";
      return;
    }
    const epoch = ++searchEpoch;
    previewEpoch++;
    searchButton.disabled = true;
    results.replaceChildren();
    searchStatus.textContent = "Checking indexed documents and live permissions…";
    void invoke<DocumentSearchHit[]>("search_document_index", { query, limit: 20 })
      .then((hits) => {
        if (!section.isConnected || epoch !== searchEpoch) return;
        searchStatus.textContent = hits.length
          ? `${hits.length} verified document matches. New/modified files require Index folder again.`
          : "No matches in the currently approved index. Approve and index a folder first, or try another phrase.";
        for (const hit of hits) {
          const item = doc.createElement("li");
          item.className = "document-search-card";
          const title = doc.createElement("strong");
          title.className = "document-result-title";
          title.textContent = hit.uri.split("/").filter(Boolean).pop() || "Indexed document";
          const source = doc.createElement("small");
          source.className = "document-result-source";
          source.textContent = `Source: ${hit.uri}`;
          const part = doc.createElement("small");
          part.className = "document-result-provenance";
          part.textContent = `Section ${hit.section || "unknown"} · revision ${hit.version_id.slice(0, 32)}`;
          const excerpt = doc.createElement("p");
          excerpt.textContent = hit.snippet;
          excerpt.className = "document-result-snippet";
          item.append(title, source, part, excerpt);
          const caveats = doc.createElement("small");
          caveats.textContent = `Extractor ${hit.extractor_revision}${hit.warnings.length ? " · " + hit.warnings.join("; ") : ""}`;
          caveats.className = "document-result-warnings";
          item.append(caveats);
          const preview = doc.createElement("button");
          preview.type = "button";
          preview.className = "quiet-button document-preview-button";
          preview.textContent = "View verified source excerpt";
          const previewStatus = doc.createElement("p");
          previewStatus.className = "document-preview-status";
          previewStatus.setAttribute("role", "status");
          const previewText = doc.createElement("pre");
          previewText.className = "document-preview-text";
          previewText.hidden = true;
          preview.addEventListener("click", () => {
            if (!section.isConnected || revocationPending) return;
            const currentEpoch = searchEpoch;
            const currentPreview = ++previewEpoch;
            preview.disabled = true;
            previewText.hidden = true;
            previewText.textContent = "";
            previewStatus.textContent = "Rechecking live folder permission and source revision…";
            void invoke<DocumentPreview>("preview_document_source", { uri: hit.uri, versionId: hit.version_id })
              .then((response) => {
                if (!section.isConnected || revocationPending || currentEpoch !== searchEpoch || currentPreview !== previewEpoch) return;
                if (response.uri !== hit.uri || response.version_id !== hit.version_id) {
                  previewStatus.textContent = "Source revision changed. Run a new search.";
                  return;
                }
                previewText.textContent = response.text;
                previewText.hidden = false;
                previewStatus.textContent = response.truncated
                  ? "Verified current source excerpt (first 4,096 characters; truncated)."
                  : "Verified current source extraction. Displayed as inert text.";
              })
              .catch((error: unknown) => {
                if (section.isConnected && currentEpoch === searchEpoch && currentPreview === previewEpoch) {
                  previewStatus.textContent = `Preview denied or source changed: ${String(error)}`;
                }
              })
              .finally(() => {
                if (section.isConnected && currentEpoch === searchEpoch && currentPreview === previewEpoch && !revocationPending) {
                  preview.disabled = false;
                }
              });
          });
          item.append(preview, previewStatus, previewText);
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
