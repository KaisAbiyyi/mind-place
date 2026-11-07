import type { Invoke } from "./reader.js";

export interface HiveNode { id: string; kind: string; label: string; canonical_key: string; source_ref: string; depth: number }
export interface HiveEdge {
  id: string; from_node: string; to_node: string; relation: string;
  origin: string; confidence_bps: number | null; source_ref: string;
  source_revision: string; locator: string;
}
export interface HiveGraph { nodes: HiveNode[]; edges: HiveEdge[]; truncated: boolean }
export interface HiveHandle { destroy(): void }
const SVG = "http://www.w3.org/2000/svg";
const MAX_NODES = 120;
const MAX_EDGES = 240;

/** Deterministic topology-based islands; unlinked evidence never acquires a synthetic edge. */
export function layoutHive(nodes: readonly HiveNode[], edges: readonly HiveEdge[] = []): Map<string, {x: number; y: number}> {
  const bounded = [...nodes.slice(0, MAX_NODES)].sort((a, b) => a.id.localeCompare(b.id));
  const lookup = new Map(bounded.map((node) => [node.id, node]));
  const adjacent = new Map(bounded.map((node) => [node.id, new Set<string>()]));
  for (const edge of edges.slice(0, MAX_EDGES)) {
    if (!lookup.has(edge.from_node) || !lookup.has(edge.to_node)) continue;
    adjacent.get(edge.from_node)!.add(edge.to_node);
    adjacent.get(edge.to_node)!.add(edge.from_node);
  }
  const seen = new Set<string>();
  const connected: HiveNode[][] = [], isolated: HiveNode[] = [];
  for (const node of bounded) {
    if (seen.has(node.id)) continue;
    const queue = [node.id], component: HiveNode[] = [];
    seen.add(node.id);
    for (let index = 0; index < queue.length; index++) {
      const id = queue[index];
      component.push(lookup.get(id)!);
      for (const next of [...adjacent.get(id)!].sort()) {
        if (!seen.has(next)) { seen.add(next); queue.push(next); }
      }
    }
    if (component.length === 1 && adjacent.get(node.id)!.size === 0) isolated.push(node);
    else connected.push(component.sort((a, b) => a.id.localeCompare(b.id)));
  }
  connected.sort((a, b) => b.length - a.length || a[0].id.localeCompare(b[0].id));
  const positions = new Map<string, {x: number; y: number}>();
  connected.forEach((component, group) => {
    const angle = connected.length === 1 ? 0 : (group / connected.length) * Math.PI * 2 - Math.PI / 2;
    const orbit = connected.length === 1 ? 0 : Math.min(180, 95 + 12 * connected.length);
    const cx = 380 + Math.cos(angle) * orbit, cy = 300 + Math.sin(angle) * orbit * .8;
    component.forEach((node, index) => {
      const phase = index * 2.399963229728653;
      const radius = index === 0 ? 0 : Math.min(135, 76 * Math.sqrt(index));
      positions.set(node.id, { x: cx + Math.cos(phase) * radius, y: cy + Math.sin(phase) * radius });
    });
  });
  isolated.forEach((node, index) => {
    const angle = ((index + .5) / Math.max(1, isolated.length)) * Math.PI * 2 - Math.PI / 2;
    const radius = 254 + (index % 3) * 7;
    positions.set(node.id, { x: 380 + Math.cos(angle) * radius, y: 300 + Math.sin(angle) * radius * .86 });
  });
  return positions;
}

/** Reject malformed native replies before inserting DOM. No HTML interpretation. */
export function validateHive(value: unknown): HiveGraph {
  if (!value || typeof value !== "object") throw Error("Invalid graph");
  const graph = value as Partial<HiveGraph>;
  if (!Array.isArray(graph.nodes) || !Array.isArray(graph.edges) ||
    graph.nodes.length > MAX_NODES || graph.edges.length > MAX_EDGES || typeof graph.truncated !== "boolean") {
    throw Error("Invalid graph bounds");
  }
  const ids = new Set<string>();
  for (const node of graph.nodes) {
    if (!node || typeof node.id !== "string" || !node.id || ids.has(node.id) ||
      !["conversation", "revision", "import"].includes(node.kind) ||
      typeof node.label !== "string" || typeof node.source_ref !== "string" ||
      typeof node.depth !== "number" || !Number.isInteger(node.depth) || node.depth < 0 || node.depth > 3 ||
      typeof node.canonical_key !== "string") throw Error("Invalid graph node");
    ids.add(node.id);
  }
  const edgeIds = new Set<string>();
  for (const edge of graph.edges) {
    if (!edge || typeof edge.id !== "string" || edgeIds.has(edge.id) ||
      !ids.has(edge.from_node) || !ids.has(edge.to_node) ||
      typeof edge.relation !== "string" || typeof edge.origin !== "string" ||
      typeof edge.source_ref !== "string" || typeof edge.source_revision !== "string" ||
      typeof edge.locator !== "string" ||
      (edge.confidence_bps !== null && (!Number.isInteger(edge.confidence_bps) || edge.confidence_bps < 8000 || edge.confidence_bps > 10000))) {
      throw Error("Invalid graph edge");
    }
    edgeIds.add(edge.id);
  }
  return graph as HiveGraph;
}

/** Archive-only graph exploration: no graph writes, no wildcard/foreign scope. */
export function mountHiveWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void, initialKey = "", onOpenArchive?: (id: string) => void): HiveHandle {
  const document = view.ownerDocument;
  let alive = true, generation = 0, graph: HiveGraph | null = null;
  let selectedId: string | null = null, zoom = 1, revisionIndex = 0;
  let panX = 0, panY = 0, focus = false;
  const history: string[] = [];
  const panel = document.createElement("section");
  panel.className = "hive-workspace";
  panel.setAttribute("aria-label", "Hive archive graph explorer");
  const header = document.createElement("header");
  header.className = "hive-heading";
  const heading = document.createElement("h1"); heading.textContent = "Hive graph";
  const back = document.createElement("button"); back.type = "button";
  back.className = "quiet-button"; back.textContent = "Back to archive";
  back.addEventListener("click", onClose);
  header.append(heading, back);
  const hint = document.createElement("p");
  hint.textContent = "Read-only archive graph. The saved conversation ID is an archive/import seed, not an indexed conversation source ID. Use the prefilled ID and Explore. Conversation and revision seed types require internal index identities, not titles or chat URLs.";
  const form = document.createElement("form"); form.className = "hive-search-form";
  const kindLabel = document.createElement("label"); kindLabel.textContent = "Seed type";
  const kind = document.createElement("select");
  for (const [value, label] of [
    ["import", "Saved archive ID (recommended)"],
    ["conversation", "Indexed source ID (advanced)"],
    ["revision", "Source ID / snapshot ID (advanced)"],
  ]) {
    const option = document.createElement("option"); option.value = value; option.textContent = label;
    kind.append(option);
  }
  kindLabel.append(kind);
  const keyLabel = document.createElement("label"); keyLabel.textContent = "Exact seed identity (not a chat title or URL)";
  const key = document.createElement("input"); key.type = "text"; key.maxLength = 4096;
  key.required = true; key.value = initialKey; key.autocomplete = "off";
  key.placeholder = "Select a saved conversation to prefill its archive ID";
  keyLabel.append(key);
  const submit = document.createElement("button"); submit.type = "submit";
  submit.className = "primary-button"; submit.textContent = "Explore";
  form.append(kindLabel, keyLabel, submit);
  const status = document.createElement("p");
  status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  const controls = document.createElement("div"); controls.className = "hive-controls";
  const zoomLabel = document.createElement("label"); zoomLabel.textContent = "Zoom";
  const zoomSlider = document.createElement("input");
  zoomSlider.type = "range"; zoomSlider.min = "60"; zoomSlider.max = "200"; zoomSlider.value = "100"; zoomSlider.step = "10";
  zoomLabel.append(zoomSlider);
  const timelineLabel = document.createElement("label");
  timelineLabel.textContent = "Evidence revision (lexical order, not a chronological date)";
  const timeline = document.createElement("input"); timeline.type = "range"; timeline.min = "0"; timeline.max = "0"; timeline.value = "0";
  timelineLabel.append(timeline);
  const revisionText = document.createElement("span"); revisionText.className = "hive-revision-label";
  controls.append(zoomLabel, timelineLabel, revisionText);
  const searchLabel = document.createElement("label"); searchLabel.textContent = "Find a node";
  const search = document.createElement("input"); search.type = "search"; search.maxLength = 256;
  search.placeholder = "Title or exact identity"; searchLabel.append(search);
  const layerLabel = document.createElement("label"); layerLabel.textContent = "Layer";
  const layer = document.createElement("select");
  for (const [value, label] of [["all", "All archive nodes"], ["conversation", "Conversations"], ["revision", "Revisions"], ["import", "Saved imports"]]) {
    const option = document.createElement("option"); option.value = value; option.textContent = label; layer.append(option);
  }
  layerLabel.append(layer);
  const originLabel = document.createElement("label"); originLabel.textContent = "Evidence";
  const origin = document.createElement("select");
  for (const [value, label] of [["all", "All relationships"], ["deterministic", "Deterministic only"], ["inferred", "Inferred only"]]) {
    const option = document.createElement("option"); option.value = value; option.textContent = label; origin.append(option);
  }
  originLabel.append(origin);
  const previous = document.createElement("button"); previous.type = "button"; previous.textContent = "Back one mind";
  previous.className = "quiet-button"; previous.disabled = true;
  const reset = document.createElement("button"); reset.type = "button"; reset.className = "quiet-button";
  reset.textContent = "Reset view";
  const refresh = document.createElement("button"); refresh.type = "button"; refresh.className = "quiet-button";
  refresh.textContent = "Refresh graph";
  controls.append(searchLabel, layerLabel, originLabel, previous, reset, refresh);
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 760 600"); svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", "Bounded archive graph. Use the node list for keyboard navigation.");
  svg.classList.add("hive-map");
  const list = document.createElement("div"); list.className = "hive-node-list";
  const detail = document.createElement("section"); detail.className = "hive-details";
  detail.setAttribute("aria-live", "polite");
  panel.append(header, hint, form, status, controls, svg, list, detail);
  hint.textContent += " Graph generation/rebuild is not exposed by this read-only API; Refresh only re-reads the current projection.";
  list.setAttribute("role", "group"); list.setAttribute("aria-label", "Keyboard-accessible graph nodes and branches");
  view.replaceChildren(panel);
  controls.hidden = true;

  const revisions = () => graph ? [...new Set(graph.edges.map(e => e.source_revision).filter(Boolean))].sort() : [];
  const activeRevision = () => revisionIndex === 0 ? null : revisions()[revisionIndex - 1] ?? null;

  const choose = (id: string) => {
    if (!graph?.nodes.some(node => node.id === id)) return;
    if (selectedId && selectedId !== id) history.push(selectedId);
    selectedId = id; focus = true; zoom = Math.max(zoom, 1.45); panX = 0; panY = 0;
    zoomSlider.value = String(Math.round(zoom * 100 / 10) * 10); render();
  };

  function render() {
    if (!alive || !graph) return;
    const versions = revisions();
    timeline.max = String(versions.length);
    revisionIndex = Math.min(revisionIndex, versions.length);
    timeline.value = String(revisionIndex);
    revisionText.textContent = revisionIndex === 0
      ? "Showing all " + graph.edges.length + " evidence edges"
      : "Revision " + revisionIndex + " of " + versions.length + ": " + versions[revisionIndex - 1];
    controls.hidden = graph.nodes.length === 0;
    svg.replaceChildren(); list.replaceChildren(); detail.replaceChildren();
    const query = search.value.trim().toLocaleLowerCase();
    const visible = graph.nodes.filter(node =>
      (layer.value === "all" || node.kind === layer.value) &&
      (!query || (node.label + " " + node.id + " " + node.canonical_key).toLocaleLowerCase().includes(query)));
    const visibleIds = new Set(visible.map(node => node.id));
    const version = activeRevision();
    const eligibleEdges = graph.edges.filter(edge => visibleIds.has(edge.from_node) && visibleIds.has(edge.to_node)
      && (version === null || edge.source_revision === version)
      && (origin.value === "all" || edge.origin === origin.value));
    const coords = layoutHive(graph.nodes, graph.edges);
    const anchor = focus && selectedId ? coords.get(selectedId) : null;
    const center = anchor ?? { x: 380, y: 300 };
    const wrapper = document.createElementNS(SVG, "g");
    wrapper.setAttribute("transform", "translate(" + (380 + panX) + " " + (300 + panY) + ") scale(" +
      zoom + ") translate(" + (-center.x) + " " + (-center.y) + ")");
    const hiveBadge = document.querySelector<HTMLElement>('[data-route-badge="hive-mind-graph"]');
    if (hiveBadge) hiveBadge.textContent = String(graph.nodes.length);
    for (const orbitRadius of [110, 215]) {
      const orbit = document.createElementNS(SVG, "ellipse");
      orbit.setAttribute("cx", "380"); orbit.setAttribute("cy", "300");
      orbit.setAttribute("rx", String(orbitRadius));
      orbit.setAttribute("ry", String(orbitRadius));
      orbit.setAttribute("class", "hive-orbit");
      wrapper.append(orbit);
    }
    const degree = new Map(visible.map(node => [node.id, 0]));
    for (const edge of eligibleEdges) {
      degree.set(edge.from_node, (degree.get(edge.from_node) ?? 0) + 1);
      degree.set(edge.to_node, (degree.get(edge.to_node) ?? 0) + 1);
      const from = coords.get(edge.from_node), to = coords.get(edge.to_node);
      if (!from || !to) continue;
      const line = document.createElementNS(SVG, "line");
      line.setAttribute("x1", String(from.x)); line.setAttribute("y1", String(from.y));
      line.setAttribute("x2", String(to.x)); line.setAttribute("y2", String(to.y));
      line.setAttribute("class", edge.origin === "inferred" ? "hive-edge hive-inferred" : "hive-edge");
      const tip = document.createElementNS(SVG, "title");
      tip.textContent = edge.relation + " · " + edge.origin + " · " + edge.source_revision;
      line.append(tip); wrapper.append(line);
    }
    for (const node of visible) {
      const pos = coords.get(node.id); if (!pos) continue;
      const circle = document.createElementNS(SVG, "circle");
      circle.setAttribute("cx", String(pos.x)); circle.setAttribute("cy", String(pos.y));
      circle.setAttribute("r", node.id === selectedId ? "10" : "7");
      circle.setAttribute("class", (node.id === selectedId ? "hive-node hive-selected" : "hive-node") +
        ((degree.get(node.id) ?? 0) === 0 ? " hive-isolated" : ""));
      const tip = document.createElementNS(SVG, "title");
      tip.textContent = node.kind + ": " + node.label;
      circle.append(tip); circle.addEventListener("click", () => choose(node.id));
      wrapper.append(circle);
      const text = document.createElementNS(SVG, "text");
      text.setAttribute("x", String(pos.x));
      text.setAttribute("y", String(pos.y + 20));
      text.setAttribute("text-anchor", "middle");
      text.setAttribute("class", node.id === selectedId ? "hive-node-label is-selected" : "hive-node-label");
      text.textContent = node.label.length > 26 ? node.label.slice(0, 25) + "…" : node.label;
      wrapper.append(text);
      const button = document.createElement("button"); button.type = "button";
      button.className = "quiet-button hive-node-choice";
      button.setAttribute("aria-pressed", String(node.id === selectedId));
      button.textContent = node.kind + " · " + node.label + ((degree.get(node.id) ?? 0) === 0 ? " · no visible links" : "");
      button.addEventListener("click", () => choose(node.id)); list.append(button);
    }
    svg.append(wrapper);
    previous.disabled = history.length === 0;
    const selected = visible.find(node => node.id === selectedId);
    if (!selected) {
      const empty = document.createElement("p"); empty.textContent = visible.length
        ? "Select a node to trace its evidence." : "No nodes match this filter. Clear search or select another layer.";
      detail.append(empty); return;
    }
    const name = document.createElement("h2"); name.textContent = selected.label;
    const identity = document.createElement("p");
    identity.textContent = "Kind: " + selected.kind + " · Source ref: " + selected.source_ref + " · Key: " + selected.canonical_key;
    detail.append(name, identity);
    if (selected.kind === "import" && onOpenArchive) {
      const open = document.createElement("button"); open.type = "button";
      open.className = "quiet-button"; open.textContent = "Open matching saved conversation";
      open.addEventListener("click", () => onOpenArchive(selected.canonical_key)); detail.append(open);
    }
    const evidence = eligibleEdges.filter(edge => edge.from_node === selected.id || edge.to_node === selected.id);
    if (evidence.length === 0) {
      const empty = document.createElement("p");
      empty.textContent = "No evidence relationships in this selection. Isolated nodes remain available without fabricated links.";
      detail.append(empty);
    }
    for (const edge of evidence) {
      const row = document.createElement("div"); row.className = "hive-evidence";
      const p = document.createElement("p");
      p.textContent = edge.relation + " · " + edge.origin +
        (edge.confidence_bps === null ? " · confidence not reported" : " · reported confidence " + edge.confidence_bps + "/10000") +
        " · source " + edge.source_ref + " · revision " + edge.source_revision + " · locator " + edge.locator;
      const other = graph.nodes.find(node => node.id === (edge.from_node === selected.id ? edge.to_node : edge.from_node));
      row.append(p);
      if (other) {
        const follow = document.createElement("button"); follow.type = "button";
        follow.className = "quiet-button"; follow.textContent = "Follow branch: " + other.label;
        follow.addEventListener("click", () => choose(other.id)); row.append(follow);
      }
      detail.append(row);
    }
  }

  zoomSlider.addEventListener("input", () => {
    zoom = Math.max(0.6, Math.min(2, Number(zoomSlider.value) / 100 || 1));
    render();
  });
  timeline.addEventListener("input", () => {
    revisionIndex = Math.max(0, Math.min(revisions().length, Number(timeline.value) || 0));
    render();
  });
  search.addEventListener("input", render);
  layer.addEventListener("change", render);
  origin.addEventListener("change", render);
  previous.addEventListener("click", () => { const id = history.pop(); if (id) { selectedId = id; focus = true; panX = 0; panY = 0; render(); } });
  reset.addEventListener("click", () => { history.length = 0; focus = false; panX = 0; panY = 0; zoom = 1; zoomSlider.value = "100"; render(); });
  refresh.addEventListener("click", () => { if (!submit.disabled) form.requestSubmit(); });
  svg.addEventListener("wheel", event => {
    event.preventDefault();
    zoom = Math.max(.6, Math.min(2, zoom + (event.deltaY < 0 ? .1 : -.1)));
    zoomSlider.value = String(Math.round(zoom * 100 / 10) * 10); render();
  }, { passive: false });
  let drag: { x: number; y: number } | null = null;
  svg.addEventListener("pointerdown", event => { drag = { x: event.clientX, y: event.clientY }; });
  svg.addEventListener("pointermove", event => {
    if (!drag) return;
    panX = Math.max(-260, Math.min(260, panX + event.clientX - drag.x));
    panY = Math.max(-260, Math.min(260, panY + event.clientY - drag.y));
    drag = { x: event.clientX, y: event.clientY }; render();
  });
  svg.addEventListener("pointerup", () => { drag = null; });
  svg.addEventListener("pointerleave", () => { drag = null; });
  form.addEventListener("submit", event => {
    event.preventDefault();
    if (!alive || submit.disabled) return;
    const identity = key.value.trim();
    if (!identity || new TextEncoder().encode(identity).length > 4096 || identity.includes("\0")) {
      status.textContent = "Enter a valid bounded seed identity."; return;
    }
    const current = ++generation;
    graph = null; selectedId = null; history.length = 0; panX = 0; panY = 0; focus = false; svg.replaceChildren(); list.replaceChildren(); detail.replaceChildren();
    controls.hidden = true;
    submit.disabled = true;
    status.textContent = "Loading a bounded read-only graph…";
    void (async () => {
      try {
        const security = await invoke<{locked: boolean; epoch: number}>("get_security_status");
        if (!alive || current !== generation || security.locked) throw Error("locked");
        const reply = validateHive(await invoke<unknown>("read_archive_hive", { kind: kind.value, key: identity, maxHops: 3 }));
        const after = await invoke<{locked: boolean; epoch: number}>("get_security_status");
        if (!alive || current !== generation || after.locked || after.epoch !== security.epoch) throw Error("expired");
        graph = reply; selectedId = reply.nodes[0]?.id ?? null; revisionIndex = 0;
        status.textContent = !reply.nodes.length ? "No indexed graph matches that exact seed. For a saved conversation ID, choose Saved archive ID, then retry after the archive index finishes refreshing."
          : reply.nodes.length + " nodes · " + reply.edges.length + " edges" +
            (reply.truncated ? " · Display truncated by graph safety limits" : "");
        render();
      } catch {
        if (alive && current === generation) {
          svg.replaceChildren(); list.replaceChildren(); detail.replaceChildren();
          controls.hidden = true;
          status.textContent = "Graph unavailable, invalid, or session locked. No graph data retained.";
        }
      } finally {
        if (alive && current === generation) submit.disabled = false;
      }
    })();
  });
  return {
    destroy: () => {
      alive = false; ++generation; graph = null; selectedId = null; history.length = 0; drag = null;
      key.value = ""; list.replaceChildren(); detail.replaceChildren(); svg.replaceChildren();
      if (panel.parentNode === view) view.replaceChildren();
    },
  };
}
