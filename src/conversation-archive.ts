import type { ConversationSummary } from "./contracts.js";
import { createUiButton, createUiStatePanel, createUiStatus, createUiTable } from "./ui-primitives.js";

export interface ArchiveFilters { query: string; provider: string; completeness: string; page: number }
export const createArchiveFilters = (): ArchiveFilters => ({ query: "", provider: "all", completeness: "all", page: 0 });

/** A bounded table over the currently authorized archive response; no invented collections or mutations. */
export function createConversationArchive(document: Document, items: readonly ConversationSummary[], filters: ArchiveFilters): HTMLElement {
  const page = document.createElement("section"); page.className = "conversation-archive-page page";
  const heading = document.createElement("header"); heading.className = "archive-page-head page-head";
  const copy = document.createElement("div"); copy.className = "archive-page-head-copy";
  const overline = document.createElement("div"); overline.className = "overline"; overline.textContent = "MULTI-PROVIDER ARCHIVE";
  const title = document.createElement("h1"); title.textContent = "Conversations Archive";
  const note = document.createElement("p"); note.className = "page-sub"; note.textContent = "Search, inspect provenance, and revisit saved multi-provider transcripts.";
  copy.append(overline, title, note);
  const setup = createUiButton(document, "Capture & import setup", { variant: "primary" }); setup.dataset.action = "open-setup";
  heading.append(copy, setup);
  const form = document.createElement("div"); form.className = "archive-filters";
  const query = document.createElement("input"); query.type = "search"; query.maxLength = 256;
  query.setAttribute("aria-label", "Filter saved conversations"); query.placeholder = "Filter by title or provider…"; query.value = filters.query;
  const provider = document.createElement("select"); provider.id = "conversation-provider-filter";
  provider.setAttribute("aria-label", "Conversation provider");
  for (const value of ["all", ...new Set(items.map(item => item.provider))]) {
    const option = document.createElement("option"); option.value = value;
    option.textContent = value === "all" ? "All AI providers" : value; provider.append(option);
  }
  provider.value = filters.provider;
  if (!provider.value) filters.provider = provider.value = "all";
  const capture = document.createElement("select"); capture.setAttribute("aria-label", "Capture completeness");
  for (const value of ["all", "complete", "partial"]) {
    const option = document.createElement("option"); option.value = value;
    option.textContent = value === "all" ? "All captures" : value === "partial" ? "Partial captures" : "Complete captures"; capture.append(option);
  }
  capture.value = filters.completeness;
  form.append(query, provider, capture);
  const content = document.createElement("div"); content.className = "archive-table-content";
  const controls = document.createElement("nav"); controls.className = "archive-pagination"; controls.setAttribute("aria-label", "Archive pages");
  const previous = createUiButton(document, "Previous"); previous.dataset.archivePage = "previous";
  const range = document.createElement("span"); range.dataset.archiveRange = ""; range.setAttribute("role", "status");
  const next = createUiButton(document, "Next"); next.dataset.archivePage = "next";
  controls.append(previous, range, next);
  const unavailable = document.createElement("p"); unavailable.className = "archive-capability-note";
  unavailable.textContent = "Starred conversations and Cold Archive are unavailable: this archive does not expose those storage actions.";
  page.append(heading, form, content, controls, unavailable);

  function render() {
    const q = filters.query.trim().toLocaleLowerCase();
    const visible = items.filter(item => (filters.provider === "all" || item.provider === filters.provider)
      && (filters.completeness === "all" || item.completeness === filters.completeness)
      && (!q || (item.title + " " + item.provider).toLocaleLowerCase().includes(q)));
    filters.page = Math.max(0, Math.min(filters.page, Math.ceil(visible.length / 50) - 1));
    const start = filters.page * 50;
    const rows = visible.slice(start, start + 50).map(item => {
      const link = createUiButton(document, item.title || "Untitled conversation", { variant: "link" });
      link.dataset.conversationId = item.id; link.dataset.archiveRow = "";
      const date = new Date(item.conversation_date || item.captured_at);
      const providerBadge = document.createElement("span");
      providerBadge.className = "tag outline mono";
      providerBadge.textContent = item.provider;
      const completenessBadge = createUiStatus(
        document,
        item.completeness,
        item.completeness === "partial" ? "partial" : "complete",
      );
      return [link, providerBadge, `${item.message_count} messages · ${item.snapshot_count} captures`,
        completenessBadge, Number.isFinite(date.getTime()) ? date.toLocaleDateString() : "Date unavailable"];
    });
    content.replaceChildren(visible.length ? createUiTable(document, {
      caption: "Saved conversations", columns: ["Conversation title", "Provider", "Capture history", "Completeness", "Last activity"], rows,
    }) : createUiStatePanel(document, { state: "empty", title: items.length ? "No conversations match" : "Your archive is empty",
      detail: items.length ? "Try another title, provider or capture filter." : "Save a conversation with the browser extension or import a transcript." }));
    previous.disabled = filters.page === 0; next.disabled = start + 50 >= visible.length;
    range.textContent = visible.length ? `${start + 1}–${Math.min(start + 50, visible.length)} of ${visible.length} conversations` : "0 conversations";
  }
  query.addEventListener("input", () => { filters.query = query.value; filters.page = 0; render(); });
  provider.addEventListener("change", () => { filters.provider = provider.value; filters.page = 0; render(); });
  capture.addEventListener("change", () => { filters.completeness = capture.value; filters.page = 0; render(); });
  previous.addEventListener("click", () => { filters.page--; render(); });
  next.addEventListener("click", () => { filters.page++; render(); });
  render(); return page;
}
