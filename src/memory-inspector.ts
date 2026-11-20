import type { Invoke } from "./reader.js";
import { mountEntityLenses, type EntityLens } from "./memory-entity-lenses.js";
import { createUiButton, createUiDrawer } from "./ui-primitives.js";

export interface MemoryClaimView {
  id: string;
  statement: string;
  asserted_by: string;
  epistemic_origin: string;
  status: string;
  revision: number;
  recorded_at_ms: number;
  evidence: Array<{ source_ref: string; evidence_kind: string }>;
}
export interface RankedMemory {
  claim: MemoryClaimView;
  disposition: string;
  score: number;
  related_claim_ids: string[];
}
export interface MemoryRevision {
  revision: number;
  recorded_at_ms: number;
  event_type: string;
  status: string;
  reason: string | null;
  superseded_by: string | null;
  evidence: Array<{ source_ref: string; evidence_kind: string }>;
}
export interface MemoryRelation {
  other_claim_id: string;
  direction: string;
  relation: string;
  source_ref: string;
  reason: string;
  recorded_at_ms: number;
  resolved_at_ms: number | null;
}
export interface MemoryDetail {
  ranked: RankedMemory;
  history: MemoryRevision[];
  relations: MemoryRelation[];
}
export interface MemoryInspectorHandle { destroy(): void; }

interface Options {
  view: HTMLElement;
  invoke: Invoke;
  copyText: (value: string) => Promise<void>;
  onClose: () => void;
  initialLens?: EntityLens;
}

const descriptions: Record<string, string> = {
  current_candidate: "Evidence-linked hypothesis — verify source before relying on it",
  contested: "Contested — must not be asserted as fact",
  superseded: "Superseded — obsolete claim",
  non_current: "Not current — historical or retracted",
  unverified: "Unverified — insufficient source evidence",
  qualified: "Qualified — additional context required",
};

function element<K extends keyof HTMLElementTagNameMap>(
  document: Document, tag: K, className: string, text?: string,
): HTMLElementTagNameMap[K] {
  const result = document.createElement(tag);
  result.className = className;
  if (text !== undefined) result.textContent = text;
  return result;
}
function timestamp(value: number): string {
  if (!Number.isFinite(value)) return "Unspecified time";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? "Unspecified time" : date.toISOString();
}
function describe(disposition: string): string {
  return descriptions[disposition] ?? "Unclassified — do not assert as fact";
}

/** Owner-unlocked, global-only memory inspection. Never render evidence as HTML
 * or treat a source reference as a permission to fetch private bytes. */
export function mountMemoryInspector({ view, invoke, copyText, onClose, initialLens }: Options): MemoryInspectorHandle {
  const document = view.ownerDocument;
  let alive = true;
  let request = 0;
  let detailRequest = 0;
  let items: RankedMemory[] = [];
  let current: MemoryDetail | null = null;
  let collection = "all";
  let quickRequest = 0;
  let drawer: HTMLDialogElement | null = null;
  let drawerReturn: HTMLElement | null = null;
  const pinned = new Set<string>(); // view-only, never authoritative storage

  view.replaceChildren();
  view.setAttribute("aria-busy", "false");
  const shell = element(document, "div", "memory-inspector");
  const top = element(document, "header", "memory-header");
  top.append(element(document, "p", "eyebrow", "Memory truth · Global scope"));
  top.append(element(document, "h1", "", "Curated Memory"));
  top.append(element(document, "p", "memory-disclaimer",
    "Claims are hypotheses, not raw evidence. Source references are identifiers only. Scoped and forgotten memories are not listed."));
  const back = element(document, "button", "quiet-button", "Back to archive");
  back.type = "button";
  back.dataset.memoryAction = "close";
  top.append(back);

  const form = element(document, "form", "memory-search");
  const label = element(document, "label", "", "Search remembered statements");
  label.htmlFor = "memory-query";
  const search = element(document, "input", "");
  search.type = "search";
  search.id = "memory-query";
  search.maxLength = 128;
  search.autocomplete = "off";
  search.placeholder = "Search global memories, insights and hypotheses…";
  const submit = element(document, "button", "quiet-button", "Search");
  submit.type = "submit";
  form.append(label, search, submit);

  const collectionHeading = element(document, "h2", "memory-collection-heading", "Memory collections");
  const collectionNote = element(document, "p", "memory-meta", "View collections group the returned global hypotheses by disposition. These are not saved collections or a complete memory census.");
  const collections = element(document, "nav", "memory-collections"); collections.setAttribute("aria-label", "Memory view collections");
  const views = element(document, "div", "memory-view-controls");
  for (const [id, label] of [["grid", "Collections grid"], ["list", "All curated memories"]]) {
    const button = createUiButton(document, label); button.dataset.memoryView = id;
    button.setAttribute("aria-pressed", String(id === "grid")); views.append(button);
  }

  const layout = element(document, "div", "memory-layout");
  const listSection = element(document, "section", "memory-results");
  listSection.setAttribute("aria-label", "Memory candidates");
  const listStatus = element(document, "p", "list-note", "Loading remembered statements…");
  listStatus.setAttribute("role", "status");
  const cards = element(document, "div", "memory-cards");
  listSection.append(listStatus, cards);
  const detailSection = element(document, "section", "memory-detail");
  detailSection.setAttribute("aria-label", "History and provenance");
  detailSection.append(element(document, "p", "list-note", "Choose a claim to inspect its history and source references."));
  layout.append(listSection, detailSection);
  shell.append(top, form, views, collectionHeading, collectionNote, collections, layout);
  view.append(shell);
  const entityLenses = mountEntityLenses(shell, invoke, initialLens);

  function showCards() {
    const focused = document.activeElement;
    const focusAttribute = ["data-memory-collection", "data-memory-claim", "data-memory-quick"]
      .find(attribute => focused?.hasAttribute(attribute));
    const focusValue = focusAttribute ? focused?.getAttribute(focusAttribute) : null;
    cards.replaceChildren();
    collections.replaceChildren();
    const memBadge = document.querySelector<HTMLElement>('[data-route-badge="curated-memory"]');
    if (memBadge) memBadge.textContent = String(items.length);
    const groups = ["all", ...new Set(items.map(item => item.disposition))];
    for (const group of groups) {
      const count = group === "all" ? items.length : items.filter(item => item.disposition === group).length;
      const button = createUiButton(document, "");
      button.append(element(document, "span", "memory-collection-count", `${count} visible memories`),
        element(document, "strong", "", group === "all" ? "All visible memories" : group.replaceAll("_", " ")),
        element(document, "span", "memory-collection-description", group === "all" ? "Inspect evidence, revisions and recorded relationships." : describe(group)));
      button.dataset.memoryCollection = group; button.setAttribute("aria-pressed", String(group === collection)); collections.append(button);
    }
    const ordered = items.filter(item => collection === "all" || item.disposition === collection)
      .sort((a, b) => Number(pinned.has(b.claim.id)) - Number(pinned.has(a.claim.id)));
    for (const entry of ordered) {
      const wrapper = element(document, "div", "memory-list-row");
      const row = element(document, "button", "memory-card");
      row.type = "button";
      row.dataset.memoryClaim = entry.claim.id;
      row.setAttribute("aria-pressed", String(entry.claim.id === current?.ranked.claim.id));
      row.append(element(document, "span", "memory-statement", entry.claim.statement));
      row.append(element(document, "span", "memory-meta",
        entry.disposition + " · revision " + entry.claim.revision +
        (pinned.has(entry.claim.id) ? " · Pinned in this view" : "")));
      const quick = createUiButton(document, "Quick Peek"); quick.dataset.memoryQuick = entry.claim.id;
      quick.setAttribute("aria-label", "Quick inspect " + entry.claim.statement.slice(0, 80));
      wrapper.append(row, quick); cards.append(wrapper);
    }
    if (!ordered.length) cards.append(element(document, "p", "memory-meta", "No visible memories in this view collection."));
    if (focusAttribute && focused && !focused.isConnected) {
      const replacement = [...shell.querySelectorAll<HTMLElement>(`[${focusAttribute}]`)]
        .find(button => button.getAttribute(focusAttribute) === focusValue);
      replacement?.focus({ preventScroll: true });
    }
  }

  function evidenceBlock(parent: HTMLElement, evidence: MemoryRevision["evidence"]) {
    if (evidence.length === 0) {
      parent.append(element(document, "p", "memory-meta", "No source references for this revision."));
    }
    for (const source of evidence) {
      parent.append(element(document, "p", "memory-evidence", source.evidence_kind + " · " + source.source_ref));
      const copy = createUiButton(document, "Copy source reference"); copy.dataset.memorySource = source.source_ref; parent.append(copy);
    }
  }

  function showDetail() {
    detailSection.replaceChildren();
    detailSection.hidden = !current;
    layout.classList.toggle("has-memory-detail", Boolean(current));
    if (!current) {
      detailSection.append(element(document, "p", "list-note", "Choose a claim to inspect its history and source references."));
      showCards();
      return;
    }
    const { ranked, history, relations } = current;
    detailSection.append(element(document, "h2", "", "Why is this remembered?"));
    detailSection.append(element(document, "p", "memory-statement", ranked.claim.statement));
    detailSection.append(element(document, "p", "memory-disposition", describe(ranked.disposition)));
    detailSection.append(element(document, "p", "memory-meta",
      "Asserted by " + ranked.claim.asserted_by + " · " + ranked.claim.epistemic_origin +
      " · " + ranked.claim.status + " · " + ranked.claim.id));
    const pin = element(document, "button", "quiet-button", pinned.has(ranked.claim.id)
      ? "Unpin from this view" : "Pin in this view");
    pin.type = "button";
    pin.dataset.memoryAction = "pin";
    detailSection.append(pin, element(document, "p", "memory-meta",
      "View-only pin; no persistent memory state is changed."));

    detailSection.append(element(document, "h3", "", "Revision history"));
    const historyLabel = element(document, "label", "memory-meta", "Inspect a recorded revision"); historyLabel.htmlFor = "memory-as-of";
    const asOf = element(document, "select", "memory-as-of"); asOf.id = "memory-as-of"; asOf.dataset.memoryAsOf = "";
    for (const revision of history) {
      const option = element(document, "option", "", `r${revision.revision} · ${timestamp(revision.recorded_at_ms)}`);
      option.value = String(revision.revision); asOf.append(option);
    }
    const historical = element(document, "section", "memory-revision"); historical.dataset.memoryHistorical = "";
    const renderHistorical = () => {
      const revision = history.find(item => String(item.revision) === asOf.value);
      historical.replaceChildren();
      if (!revision) return;
      historical.append(element(document, "p", "", `Recorded r${revision.revision} · ${revision.status} · ${revision.event_type}`));
      if (revision.reason) historical.append(element(document, "p", "", revision.reason));
      historical.append(element(document, "p", "memory-meta", "Historical metadata from the authorized timeline. The inspection API does not return historical statement text or an as-of ranking."));
      evidenceBlock(historical, revision.evidence);
    };
    asOf.addEventListener("change", renderHistorical); renderHistorical(); detailSection.append(historyLabel, asOf, historical);
    let previous = "not yet recorded";
    for (const revision of history) {
      const section = element(document, "section", "memory-revision");
      section.append(element(document, "h4", "",
        "r" + revision.revision + " · " + previous + " → " + revision.status));
      section.append(element(document, "p", "memory-meta",
        revision.event_type + " · " + timestamp(revision.recorded_at_ms)));
      if (revision.reason) section.append(element(document, "p", "", revision.reason));
      if (revision.superseded_by) section.append(element(document, "p", "memory-meta",
        "Successor ID: " + revision.superseded_by));
      evidenceBlock(section, revision.evidence);
      detailSection.append(section);
      previous = revision.status;
    }
    detailSection.append(element(document, "h3", "", "Conflict & relation viewer"));
    if (relations.length === 0) detailSection.append(element(document, "p", "memory-meta", "No registered relations."));
    for (const relation of relations) {
      const section = element(document, "section", "memory-revision");
      section.append(element(document, "p", "memory-statement",
        relation.direction + " · " + relation.relation + " · " + relation.other_claim_id));
      const related = createUiButton(document, "Inspect related memory"); related.dataset.memoryRelated = relation.other_claim_id; section.append(related);
      section.append(element(document, "p", "memory-meta", relation.reason));
      section.append(element(document, "p", "memory-evidence",
        "Proof reference: " + relation.source_ref + " · " + timestamp(relation.recorded_at_ms)));
      if (relation.resolved_at_ms !== null) section.append(element(document, "p", "memory-meta",
        "Relation resolved at " + timestamp(relation.resolved_at_ms)));
      detailSection.append(section);
    }

    detailSection.append(element(document, "h3", "", "Manual correction draft"));
    const correctionLabel = element(document, "label", "memory-meta",
      "Proposed corrected statement (not automatically saved)");
    correctionLabel.htmlFor = "memory-correction";
    const correction = element(document, "textarea", "memory-correction");
    correction.id = "memory-correction";
    correction.maxLength = 512;
    correction.rows = 3;
    const copy = element(document, "button", "quiet-button", "Copy correction request");
    copy.type = "button";
    copy.dataset.memoryAction = "copy-correction";
    const correctionStatus = element(document, "p", "memory-meta");
    correctionStatus.id = "memory-correction-status";
    correctionStatus.setAttribute("role", "status");
    detailSection.append(correctionLabel, correction, copy, correctionStatus);
    detailSection.append(element(document, "p", "memory-disclaimer",
      "This is a draft only. Authorized evidence-linked writes must use the memory write capability; copying does not change authoritative state."));
    const lifecycle = element(document, "section", "memory-lifecycle"); lifecycle.append(element(document, "h3", "", "Memory lifecycle"));
    for (const action of ["create", "update", "forget"]) {
      const button = createUiButton(document, action[0].toUpperCase() + action.slice(1), { disabled: true });
      button.dataset.memoryWrite = action; button.title = "No accepted desktop memory-write authority is available."; lifecycle.append(button);
    }
    lifecycle.append(element(document, "p", "memory-meta", "Create, update and forget require an accepted desktop write-authority contract. No memory or privacy state is changed here."));
    detailSection.append(lifecycle);
    showCards();
  }

  function closeQuick(restore = true) {
    ++quickRequest;
    if (drawer) { const old = drawer; drawer = null; if (old.open && typeof old.close === "function") old.close(); old.replaceChildren(); old.remove(); }
    if (restore && drawerReturn?.isConnected) drawerReturn.focus();
    drawerReturn = null;
  }

  async function openQuick(id: string, trigger: HTMLElement) {
    closeQuick(false); drawerReturn = trigger;
    const version = ++quickRequest;
    const panel = createUiDrawer(document, { id: "memory-quick", title: "Provenance Quick Inspector", className: "memory-quick-drawer" });
    drawer = panel; const body = panel.querySelector<HTMLElement>(".ui-dialog-body")!;
    body.textContent = "Checking authorized memory provenance…"; shell.append(panel);
    if (typeof panel.showModal === "function") panel.showModal(); else panel.setAttribute("open", "");
    panel.querySelector<HTMLButtonElement>("button")?.focus();
    panel.addEventListener("cancel", event => { event.preventDefault(); closeQuick(); });
    panel.addEventListener("keydown", event => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); closeQuick(); }
      if (event.key === "Tab") {
        const controls = [...panel.querySelectorAll<HTMLElement>("button:not(:disabled), a[href], input, select, textarea")];
        const first = controls[0], last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
    });
    panel.querySelector<HTMLButtonElement>(".ui-dialog-close")!.addEventListener("click", event => { event.stopPropagation(); closeQuick(); });
    try {
      const result = await invoke<MemoryDetail | null>("get_memory_inspection", { claimId: id });
      if (!alive || version !== quickRequest || drawer !== panel) return;
      if (!result || result.ranked.claim.id !== id) throw new Error("Unavailable");
      body.replaceChildren(element(document, "h3", "", result.ranked.claim.statement),
        element(document, "p", "memory-disposition", describe(result.ranked.disposition)),
        element(document, "p", "memory-meta", `Asserted by ${result.ranked.claim.asserted_by} · ${result.ranked.claim.epistemic_origin} · r${result.ranked.claim.revision}`));
      evidenceBlock(body, result.ranked.claim.evidence);
      const full = createUiButton(document, "Open full history & conflicts");
      full.addEventListener("click", () => { closeQuick(false); void selectClaim(id); }); body.append(full);
    } catch {
      if (alive && version === quickRequest) body.textContent = "Memory unavailable, forgotten or no longer authorized.";
    }
  }

  async function selectClaim(id: string) {
    const version = ++detailRequest;
    current = null;
    detailSection.hidden = false;
    layout.classList.add("has-memory-detail");
    detailSection.replaceChildren(element(document, "p", "list-note", "Loading provenance…"));
    try {
      const result = await invoke<MemoryDetail | null>("get_memory_inspection", { claimId: id });
      if (!alive || version !== detailRequest) return;
      if (result && result.ranked.claim.id !== id) throw new Error("Memory response identity mismatch");
      current = result;
      if (current) showDetail();
      else detailSection.replaceChildren(element(document, "p", "list-error", "Memory unavailable or forgotten."));
    } catch {
      if (!alive || version !== detailRequest) return;
      detailSection.replaceChildren(element(document, "p", "list-error", "Provenance unavailable. No memory changes were made."));
    }
  }

  async function load() {
    const version = ++request;
    ++detailRequest;
    current = null;
    closeQuick(false);
    items = []; collection = "all";
    showDetail();
    listStatus.textContent = "Loading remembered statements…";
    try {
      const response = await invoke<RankedMemory[]>("list_memory_inspection", { query: search.value.trim() });
      if (!alive || version !== request) return;
      if (!Array.isArray(response)) throw new Error("Invalid memory result");
      items = response.slice(0, 50);
      listStatus.textContent = items.length
        ? "Showing up to 50 global claims · all statuses included"
        : "No visible matching claims. Forgotten and scoped memories remain undisclosed.";
      showCards();
    } catch {
      if (!alive || version !== request) return;
      items = [];
      listStatus.textContent = "Memory inspection unavailable. Unlock the app or retry.";
      showCards();
    }
  }

  const submitHandler = (event: Event) => {
    event.preventDefault();
    if (!alive) return;
    void load();
  };
  const clickHandler = (event: Event) => {
    const target = event.target;
    if (!alive || !(target instanceof document.defaultView!.Element)) return;
    const action = target.closest<HTMLElement>("[data-memory-action]")?.dataset.memoryAction;
    const group = target.closest<HTMLElement>("[data-memory-collection]")?.dataset.memoryCollection;
    if (group) { collection = group; showCards(); return; }
    const viewMode = target.closest<HTMLElement>("[data-memory-view]")?.dataset.memoryView;
    if (viewMode) {
      collections.hidden = collectionHeading.hidden = collectionNote.hidden = viewMode === "list";
      collection = "all"; showCards();
      for (const button of views.querySelectorAll("button")) button.setAttribute("aria-pressed", String((button as HTMLElement).dataset.memoryView === viewMode));
      return;
    }
    const quick = target.closest<HTMLElement>("[data-memory-quick]");
    if (quick?.dataset.memoryQuick) { void openQuick(quick.dataset.memoryQuick, quick); return; }
    const source = target.closest<HTMLElement>("[data-memory-source]")?.dataset.memorySource;
    if (source) { void copyText(source).catch(() => {}); return; }
    const related = target.closest<HTMLElement>("[data-memory-related]")?.dataset.memoryRelated;
    if (related) { void selectClaim(related); return; }
    if (action === "close") onClose();
    else if (action === "pin" && current) {
      const id = current.ranked.claim.id;
      if (pinned.has(id)) pinned.delete(id);
      else pinned.add(id);
      showDetail();
    } else if (action === "copy-correction" && current) {
      const input = detailSection.querySelector<HTMLTextAreaElement>("#memory-correction");
      const status = detailSection.querySelector<HTMLElement>("#memory-correction-status");
      const proposed = input?.value.trim() ?? "";
      if (!status) return;
      if (!proposed) { status.textContent = "Enter a proposed correction first."; return; }
      const text = "Manual memory correction request (not applied)\nClaim: " +
        current.ranked.claim.id + "\nExisting: " + current.ranked.claim.statement +
        "\nProposed: " + proposed + "\nEvidence must be provided to an authorized writer.";
      void copyText(text).then(() => {
        if (alive && status.isConnected) status.textContent = "Correction request copied; memory unchanged.";
      }).catch(() => {
        if (alive && status.isConnected) status.textContent = "Clipboard unavailable; memory unchanged.";
      });
    } else {
      const id = target.closest<HTMLElement>("[data-memory-claim]")?.dataset.memoryClaim;
      if (id) void selectClaim(id);
    }
  };
  form.addEventListener("submit", submitHandler);
  shell.addEventListener("click", clickHandler);
  void load();

  return {
    destroy() {
      closeQuick(false);
      entityLenses.destroy();
      alive = false;
      ++request;
      ++detailRequest;
      pinned.clear();
      current = null;
      items = [];
      form.removeEventListener("submit", submitHandler);
      shell.removeEventListener("click", clickHandler);
      view.replaceChildren();
    },
  };
}
