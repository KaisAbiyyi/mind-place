import type { Invoke } from "./reader.js";
import type { Conversation } from "./contracts.js";
import type { DocumentSearchHit } from "./document-roots.js";

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
export interface LibraryHandle { destroy(): void }

type Filter = "all" | "conversations" | "files";

/** Read-only, unlock-lifetime-scoped discovery across already-permitted native indexes.
 * Native endpoints are capped but not paginated; do not imply full archive enumeration. */
export function mountLibraryWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void): LibraryHandle {
  const doc = view.ownerDocument;
  let alive = true;
  let epoch = 0;
  let previewEpoch = 0;
  let filter: Filter = "all";
  let archiveHits: ArchiveSearchHit[] = [];
  let fileHits: DocumentSearchHit[] = [];
  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string) => {
    const el = doc.createElement(tag);
    if (text !== undefined) el.textContent = text;
    return el;
  };
  const shell = make("section"); shell.className = "knowledge-library";
  const heading = make("h1", "Search your library");
  const note = make("p", "Search saved message indexes and currently authorized document roots. Matches are bounded; this is not a full archive listing.");
  note.className = "list-note";
  const close = make("button", "Back to conversations"); close.type = "button"; close.className = "quiet-button"; close.dataset.libraryAction = "close";
  const form = make("form"); form.className = "knowledge-library-form";
  const label = make("label", "Search phrase"); label.htmlFor = "library-query";
  const query = make("input"); query.type = "search"; query.id = "library-query"; query.maxLength = 256; query.autocomplete = "off";
  const submit = make("button", "Search"); submit.type = "submit"; submit.className = "primary-button";
  form.append(label, query, submit);
  const tabs = make("nav"); tabs.className = "knowledge-library-filters"; tabs.setAttribute("aria-label", "Source filter");
  for (const [id, caption] of [["all", "All"], ["conversations", "Conversations"], ["files", "Files"]] as const) {
    const b = make("button", caption); b.type = "button"; b.dataset.libraryFilter = id; b.setAttribute("aria-pressed", String(id === filter)); tabs.append(b);
  }
  const status = make("p"); status.className = "list-note"; status.setAttribute("role", "status");
  const results = make("div"); results.className = "knowledge-library-results"; results.setAttribute("aria-label", "Search results");
  const preview = make("section"); preview.className = "knowledge-library-preview"; preview.setAttribute("aria-label", "Source and version preview");
  preview.append(make("p", "Choose a match to preview its provenance."));
  shell.append(heading, note, close, form, tabs, status, results, preview);
  view.replaceChildren(shell); view.setAttribute("aria-busy", "false");

  function renderResults() {
    results.replaceChildren();
    for (const [index, hit] of archiveHits.entries()) {
      if (filter === "files") break;
      const button = make("button"); button.type = "button"; button.className = "knowledge-library-hit";
      button.dataset.libraryArchive = String(index);
      button.append(make("strong", hit.title || "Untitled conversation"), make("small", "Conversation · " + hit.role + " · " + hit.kind));
      button.append(make("span", hit.snippet)); results.append(button);
    }
    for (const [index, hit] of fileHits.entries()) {
      if (filter === "conversations") break;
      const button = make("button"); button.type = "button"; button.className = "knowledge-library-hit";
      button.dataset.libraryFile = String(index);
      button.append(make("strong", hit.uri), make("small", "Document · " + hit.section + " · version " + hit.version_id));
      button.append(make("span", hit.snippet)); results.append(button);
    }
    if (!results.childElementCount) results.append(make("p", "No visible matches. Try a different phrase or source filter."));
  }

  async function search() {
    const q = query.value.trim();
    ++previewEpoch;
    preview.replaceChildren(make("p", "Choose a match to preview its provenance."));
    if (!q || q.length > 256) { status.textContent = "Enter a search phrase (1–256 characters)."; return; }
    const version = ++epoch;
    submit.disabled = true; status.textContent = "Searching currently accessible indexes…";
    results.replaceChildren();
    const [archives, files] = await Promise.allSettled([
      invoke<ArchiveSearchHit[]>("search_archive_index", { query: q, includeHistory: true, limit: 40 }),
      invoke<DocumentSearchHit[]>("search_document_index", { query: q, limit: 40 }),
    ]);
    if (!alive || version !== epoch) return;
    archiveHits = archives.status === "fulfilled" && Array.isArray(archives.value) ? archives.value.slice(0, 40) : [];
    fileHits = files.status === "fulfilled" && Array.isArray(files.value) ? files.value.slice(0, 40) : [];
    const failed = [archives.status !== "fulfilled" ? "conversation index" : "", files.status !== "fulfilled" ? "document index" : ""].filter(Boolean);
    status.textContent = archiveHits.length + " conversation matches · " + fileHits.length + " file matches (up to 40 per index)." +
      (failed.length ? " Unavailable: " + failed.join(", ") + ". Results may be incomplete." : "");
    submit.disabled = false; renderResults();
  }

  async function previewArchive(hit: ArchiveSearchHit) {
    const version = ++previewEpoch;
    preview.replaceChildren(make("p", "Checking selected snapshot…"));
    try {
      if (!hit.archive_id || !hit.snapshot_id) throw new Error("Missing archive identity");
      const result = await invoke<Conversation>("read_conversation", { id: hit.archive_id, snapshotId: hit.snapshot_id });
      if (!alive || version !== previewEpoch) return;
      if (result.summary.id !== hit.archive_id || (result.snapshot_id && result.snapshot_id !== hit.snapshot_id)) {
        throw new Error("Returned source/version differs from selected index result");
      }
      // The index stores source titles with the reserved @title pseudo-message ID.
      // Resolve title matches against the exact authenticated capture, not messages.
      const titleHit = hit.kind === "title" && hit.message_id === "@title";
      const matching = titleHit ? undefined : result.capture.messages.find((message) => message.id === hit.message_id);
      if (!titleHit && !matching) throw new Error("Selected source message not available in this snapshot");
      const sourceText = titleHit ? result.capture.title : matching!.text;
      preview.replaceChildren(
        make("h2", result.summary.title || "Conversation source"),
        make("p", "Archive ID: " + hit.archive_id),
        make("p", "Snapshot: " + hit.snapshot_id + (titleHit ? " · title" : " · message " + hit.message_id)),
        make("p", "Provider: " + result.summary.provider + " · captured " + result.capture.captured_at),
        make("pre", sourceText.slice(0, 1800) + (sourceText.length > 1800 ? "…" : "")),
      );
    } catch {
      if (alive && version === previewEpoch) preview.replaceChildren(make("p", "Source preview unavailable or no longer authorized. Refresh the search."));
    }
  }
  function previewFile(hit: DocumentSearchHit) {
    ++previewEpoch;
    // Metadata + verified search snippet only: no raw filesystem or unauthorized full-document read.
    preview.replaceChildren(
      make("h2", "Indexed document excerpt"),
      make("p", "Source: " + hit.uri),
      make("p", "Version: " + hit.version_id + " · section: " + hit.section),
      make("p", "Extractor: " + hit.extractor_revision),
      make("pre", hit.snippet),
      make("p", hit.warnings.length ? "Warnings: " + hit.warnings.join("; ") : "Excerpt only; indexing and live root permission checks still apply."),
    );
  }
  const submitHandler = (event: Event) => { event.preventDefault(); if (alive) void search(); };
  const clickHandler = (event: Event) => {
    const target = event.target;
    if (!alive || !(target instanceof doc.defaultView!.Element)) return;
    if (target.closest<HTMLElement>("[data-library-action='close']")) { onClose(); return; }
    const tab = target.closest<HTMLButtonElement>("[data-library-filter]");
    if (tab) {
      filter = tab.dataset.libraryFilter as Filter;
      for (const button of tabs.querySelectorAll<HTMLButtonElement>("[data-library-filter]")) button.setAttribute("aria-pressed", String(button === tab));
      renderResults(); return;
    }
    const archive = target.closest<HTMLElement>("[data-library-archive]");
    if (archive) { const hit = archiveHits[Number(archive.dataset.libraryArchive)]; if (hit) void previewArchive(hit); return; }
    const file = target.closest<HTMLElement>("[data-library-file]");
    if (file) { const hit = fileHits[Number(file.dataset.libraryFile)]; if (hit) previewFile(hit); }
  };
  form.addEventListener("submit", submitHandler);
  shell.addEventListener("click", clickHandler);
  return { destroy() {
    alive = false; ++epoch; ++previewEpoch;
    archiveHits = []; fileHits = []; query.value = "";
    form.removeEventListener("submit", submitHandler);
    shell.removeEventListener("click", clickHandler);
    view.replaceChildren();
  } };
}
