import type { Invoke } from "./reader.js";

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
export function mountMemoryInspector({ view, invoke, copyText, onClose }: Options): MemoryInspectorHandle {
  const document = view.ownerDocument;
  let alive = true;
  let request = 0;
  let detailRequest = 0;
  let items: RankedMemory[] = [];
  let current: MemoryDetail | null = null;
  const pinned = new Set<string>(); // view-only, never authoritative storage

  view.replaceChildren();
  view.setAttribute("aria-busy", "false");
  const shell = element(document, "div", "memory-inspector");
  const top = element(document, "header", "memory-header");
  top.append(element(document, "p", "eyebrow", "Memory truth · Global scope"));
  top.append(element(document, "h1", "", "History, conflicts & provenance"));
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
  const submit = element(document, "button", "quiet-button", "Search");
  submit.type = "submit";
  form.append(label, search, submit);

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
  shell.append(top, form, layout);
  view.append(shell);

  function showCards() {
    cards.replaceChildren();
    const ordered = [...items].sort((a, b) => Number(pinned.has(b.claim.id)) - Number(pinned.has(a.claim.id)));
    for (const entry of ordered) {
      const row = element(document, "button", "memory-card");
      row.type = "button";
      row.dataset.memoryClaim = entry.claim.id;
      row.setAttribute("aria-pressed", String(entry.claim.id === current?.ranked.claim.id));
      row.append(element(document, "span", "memory-statement", entry.claim.statement));
      row.append(element(document, "span", "memory-meta",
        entry.disposition + " · revision " + entry.claim.revision +
        (pinned.has(entry.claim.id) ? " · Pinned in this view" : "")));
      cards.append(row);
    }
  }

  function evidenceBlock(parent: HTMLElement, evidence: MemoryRevision["evidence"]) {
    if (evidence.length === 0) {
      parent.append(element(document, "p", "memory-meta", "No source references for this revision."));
    }
    for (const source of evidence) {
      parent.append(element(document, "p", "memory-evidence", source.evidence_kind + " · " + source.source_ref));
    }
  }

  function showDetail() {
    detailSection.replaceChildren();
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
    showCards();
  }

  async function selectClaim(id: string) {
    const version = ++detailRequest;
    current = null;
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
    showDetail();
    listStatus.textContent = "Loading remembered statements…";
    try {
      const response = await invoke<RankedMemory[]>("list_memory_inspection", { query: search.value.trim() });
      if (!alive || version !== request) return;
      items = response;
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
