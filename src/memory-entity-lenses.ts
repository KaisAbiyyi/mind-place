import type { Invoke } from "./reader.js";
import type { RankedMemory, MemoryDetail } from "./memory-inspector.js";

export type EntityLens = "projects" | "people" | "topics";
export interface EntityLensHandle { destroy(): void }

/** Explicit name-based investigation lenses over visible GLOBAL claims only.
 * This does not create or infer authoritative entity identity, claim scope or relationships. */
export function mountEntityLenses(parent: HTMLElement, invoke: Invoke): EntityLensHandle {
  const doc = parent.ownerDocument;
  let alive = true, queryEpoch = 0, detailEpoch = 0;
  let lens: EntityLens = "projects";
  let claims: RankedMemory[] = [];
  const el = <K extends keyof HTMLElementTagNameMap>(name: K, text?: string) => {
    const result = doc.createElement(name);
    if (text !== undefined) result.textContent = text;
    return result;
  };
  const section = el("section"); section.className = "entity-lenses"; section.setAttribute("aria-label", "Project, person and topic investigation");
  section.append(el("h2", "Projects, people & topics"));
  const disclaimer = el("p", "Read-only investigation by a name you provide. Results are visible global memory hypotheses, NOT a canonical entity registry or validated identity. Scoped/forgotten claims remain hidden.");
  disclaimer.className = "memory-disclaimer"; section.append(disclaimer);
  const tabs = el("nav"); tabs.setAttribute("aria-label", "Investigation lens"); tabs.className = "entity-lens-tabs";
  for (const kind of ["projects", "people", "topics"] as const) {
    const b = el("button", kind === "projects" ? "Projects" : kind === "people" ? "People" : "Topics");
    b.type = "button"; b.dataset.entityLens = kind; b.setAttribute("aria-pressed", String(kind === lens)); tabs.append(b);
  }
  const form = el("form"); form.className = "entity-lens-form";
  const label = el("label", "Project name"); label.htmlFor = "entity-lens-name";
  const input = el("input"); input.id = "entity-lens-name"; input.type = "search"; input.maxLength = 128; input.autocomplete = "off"; input.required = true;
  const searchButton = el("button", "Inspect mentions"); searchButton.type = "submit"; searchButton.className = "quiet-button";
  form.append(label, input, searchButton);
  const status = el("p", "Enter a name to find related global claims."); status.className = "list-note"; status.setAttribute("role", "status");
  const layout = el("div"); layout.className = "entity-lens-layout";
  const list = el("div"); list.className = "entity-lens-results"; list.setAttribute("aria-label", "Matching memory hypotheses");
  const detail = el("section"); detail.className = "entity-lens-detail"; detail.setAttribute("aria-label", "Claim history, sources and relationships");
  detail.append(el("p", "Select a matching hypothesis to inspect its timeline and source references."));
  layout.append(list, detail); section.append(tabs, form, status, layout); parent.append(section);

  function renderMatches() {
    list.replaceChildren();
    for (const [i, entry] of claims.slice(0, 50).entries()) {
      const button = el("button"); button.type = "button"; button.className = "entity-lens-item"; button.dataset.entityClaim = String(i);
      button.append(el("strong", entry.claim.statement), el("small", entry.disposition + " · claim " + entry.claim.id));
      list.append(button);
    }
    if (!list.childElementCount) list.append(el("p", "No visible global claims match this name."));
  }

  async function searchName() {
    const name = input.value.trim();
    ++detailEpoch;
    detail.replaceChildren(el("p", "Select a hypothesis to inspect evidence."));
    if (!name || name.length > 128) { status.textContent = "Enter a name (1–128 characters)."; return; }
    const current = ++queryEpoch; searchButton.disabled = true;
    status.textContent = "Checking currently visible global memory hypotheses…";
    list.replaceChildren();
    try {
      const response = await invoke<RankedMemory[]>("list_memory_inspection", { query: name });
      if (!alive || current !== queryEpoch) return;
      if (!Array.isArray(response)) throw new Error("Invalid memory result");
      claims = response.slice(0, 50);
      status.textContent = claims.length + " visible global matches for " + lens + " name “" + name + "” (maximum 50).";
      renderMatches();
    } catch {
      if (!alive || current !== queryEpoch) return;
      claims = []; list.replaceChildren();
      status.textContent = "Memory inspection unavailable; no private or scoped information was shown.";
    } finally {
      if (alive && current === queryEpoch) searchButton.disabled = false;
    }
  }

  async function showClaim(index: number) {
    const claim = claims[index];
    if (!claim) return;
    const version = ++detailEpoch; detail.replaceChildren(el("p", "Checking provenance…"));
    try {
      const result = await invoke<MemoryDetail | null>("get_memory_inspection", { claimId: claim.claim.id });
      if (!alive || version !== detailEpoch) return;
      if (!result || result.ranked.claim.id !== claim.claim.id) throw new Error("Claim unavailable or identity changed");
      detail.replaceChildren();
      detail.append(el("h3", "Hypothesis and identity"), el("p", result.ranked.claim.statement));
      detail.append(el("p", "Status: " + result.ranked.disposition + " · asserted by " + result.ranked.claim.asserted_by + " · claim " + result.ranked.claim.id));
      const timeline = el("ol"); timeline.className = "entity-lens-timeline";
      for (const rev of [...result.history].sort((a,b) => a.recorded_at_ms - b.recorded_at_ms || a.revision - b.revision).slice(0, 100)) {
        const item = el("li");
        const date = Number.isFinite(rev.recorded_at_ms) ? new Date(rev.recorded_at_ms).toISOString() : "Time unavailable";
        item.append(el("strong", "Revision " + rev.revision + " · " + rev.event_type + " · " + rev.status));
        item.append(el("p", date + (rev.reason ? " · " + rev.reason : "")));
        if (rev.superseded_by) item.append(el("p", "Successor: " + rev.superseded_by));
        for (const source of rev.evidence.slice(0, 40)) item.append(el("p", source.evidence_kind + " · " + source.source_ref));
        timeline.append(item);
      }
      detail.append(el("h4", "Revision timeline"), timeline);
      const relationList = el("ul");
      for (const relation of result.relations.slice(0, 100)) {
        const li = el("li", relation.relation + " → " + relation.other_claim_id + " · " + relation.reason + " · ref " + relation.source_ref);
        relationList.append(li);
      }
      detail.append(el("h4", "Recorded source relationships"), relationList);
      if (!result.relations.length) detail.append(el("p", "No recorded relationships."));
      detail.append(el("p", "Source IDs are evidence pointers, not grants to read underlying content."));
    } catch {
      if (alive && version === detailEpoch) detail.replaceChildren(el("p", "This claim is unavailable, forgotten or no longer authorized."));
    }
  }

  const submitHandler = (event: Event) => { event.preventDefault(); if (alive) void searchName(); };
  const clickHandler = (event: Event) => {
    const target = event.target;
    if (!alive || !(target instanceof doc.defaultView!.Element)) return;
    const tab = target.closest<HTMLButtonElement>("[data-entity-lens]");
    if (tab) {
      lens = tab.dataset.entityLens as EntityLens;
      label.textContent = lens === "projects" ? "Project name" : lens === "people" ? "Person name" : "Topic name";
      tabs.querySelectorAll<HTMLButtonElement>("[data-entity-lens]").forEach(b => b.setAttribute("aria-pressed", String(b === tab)));
      ++queryEpoch; ++detailEpoch; claims = []; input.value = ""; list.replaceChildren();
      detail.replaceChildren(el("p", "Select a matching hypothesis to inspect evidence."));
      status.textContent = "Enter a " + label.textContent.toLowerCase() + " to inspect global claims.";
      searchButton.disabled = false; input.focus(); return;
    }
    const item = target.closest<HTMLElement>("[data-entity-claim]");
    if (item) void showClaim(Number(item.dataset.entityClaim));
  };
  form.addEventListener("submit", submitHandler);
  section.addEventListener("click", clickHandler);
  return { destroy() {
    alive = false; ++queryEpoch; ++detailEpoch;
    claims = []; input.value = ""; form.removeEventListener("submit", submitHandler);
    section.removeEventListener("click", clickHandler); section.remove();
  } };
}
