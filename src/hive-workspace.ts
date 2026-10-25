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

/** Stable non-force layout; no timer/network and a fixed DOM budget. */
export function layoutHive(nodes: readonly HiveNode[]): Map<string, {x: number; y: number}> {
  const groups = new Map<number, HiveNode[]>();
  for (const node of nodes.slice(0, MAX_NODES)) {
    const d = Math.max(0, Math.min(3, Math.trunc(node.depth)));
    groups.set(d, [...(groups.get(d) ?? []), node]);
  }
  const positions = new Map<string, {x: number; y: number}>();
  for (const [depth, rows] of groups) {
    rows.sort((a, b) => a.id.localeCompare(b.id));
    rows.forEach((node, i) => {
      const a = ((i + (depth % 2) / 2) / Math.max(1, rows.length)) * Math.PI * 2 - Math.PI / 2;
      const radius = depth === 0 ? 38 : 75 + depth * 76;
      positions.set(node.id, { x: 380 + Math.cos(a) * radius, y: 300 + Math.sin(a) * radius });
    });
  }
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
export function mountHiveWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void, initialKey = ""): HiveHandle {
  const document = view.ownerDocument;
  let alive = true, generation = 0, graph: HiveGraph | null = null;
  let selectedId: string | null = null, zoom = 1, revisionIndex = 0;
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
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 760 600"); svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", "Bounded archive graph. Use the node list for keyboard navigation.");
  svg.classList.add("hive-map");
  const list = document.createElement("div"); list.className = "hive-node-list";
  const detail = document.createElement("section"); detail.className = "hive-details";
  detail.setAttribute("aria-live", "polite");
  panel.append(header, hint, form, status, controls, svg, list, detail);
  view.replaceChildren(panel);
  controls.hidden = true;

  const revisions = () => graph ? [...new Set(graph.edges.map(e => e.source_revision).filter(Boolean))].sort() : [];
  const activeRevision = () => revisionIndex === 0 ? null : revisions()[revisionIndex - 1] ?? null;

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
    const coords = layoutHive(graph.nodes);
    const wrapper = document.createElementNS(SVG, "g");
    wrapper.setAttribute("transform", "translate(380 300) scale(" + zoom + ") translate(-380 -300)");
    const version = activeRevision();
    for (const edge of graph.edges) {
      if (version !== null && edge.source_revision !== version) continue;
      const from = coords.get(edge.from_node), to = coords.get(edge.to_node);
      if (!from || !to) continue;
      const line = document.createElementNS(SVG, "line");
      line.setAttribute("x1", String(from.x)); line.setAttribute("y1", String(from.y));
      line.setAttribute("x2", String(to.x)); line.setAttribute("y2", String(to.y));
      line.setAttribute("class", edge.origin === "inferred" ? "hive-edge hive-inferred" : "hive-edge");
      wrapper.append(line);
    }
    for (const node of graph.nodes) {
      const pos = coords.get(node.id);
      if (!pos) continue;
      const circle = document.createElementNS(SVG, "circle");
      circle.setAttribute("cx", String(pos.x)); circle.setAttribute("cy", String(pos.y));
      circle.setAttribute("r", node.id === selectedId ? "10" : "7");
      circle.setAttribute("class", node.id === selectedId ? "hive-node hive-selected" : "hive-node");
      const tip = document.createElementNS(SVG, "title");
      tip.textContent = node.kind + ": " + node.label;
      circle.append(tip); wrapper.append(circle);
      const button = document.createElement("button"); button.type = "button";
      button.className = "quiet-button hive-node-choice";
      button.setAttribute("aria-pressed", String(node.id === selectedId));
      button.textContent = node.kind + " · " + node.label;
      button.addEventListener("click", () => { selectedId = node.id; render(); });
      list.append(button);
    }
    svg.append(wrapper);
    const selected = graph.nodes.find(x => x.id === selectedId);
    if (!selected) return;
    const name = document.createElement("h2"); name.textContent = selected.label;
    const identity = document.createElement("p");
    identity.textContent = "Kind: " + selected.kind + " · Source ref: " + selected.source_ref + " · Key: " + selected.canonical_key;
    detail.append(name, identity);
    const evidence = graph.edges.filter(e =>
      (e.from_node === selected.id || e.to_node === selected.id) &&
      (version === null || e.source_revision === version)).slice(0, MAX_EDGES);
    if (evidence.length === 0) {
      const empty = document.createElement("p"); empty.textContent = "No edges in this revision selection."; detail.append(empty);
    }
    for (const edge of evidence) {
      const p = document.createElement("p"); p.className = "hive-evidence";
      p.textContent = edge.relation + " · " + edge.origin +
        (edge.confidence_bps === null ? "" : " (reported confidence " + edge.confidence_bps + "/10000)") +
        " · source " + edge.source_ref + " · revision " + edge.source_revision + " · locator " + edge.locator;
      detail.append(p);
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
  form.addEventListener("submit", event => {
    event.preventDefault();
    if (!alive || submit.disabled) return;
    const identity = key.value.trim();
    if (!identity || new TextEncoder().encode(identity).length > 4096 || identity.includes("\0")) {
      status.textContent = "Enter a valid bounded seed identity."; return;
    }
    const current = ++generation;
    graph = null; selectedId = null; svg.replaceChildren(); list.replaceChildren(); detail.replaceChildren();
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
      alive = false; ++generation; graph = null; selectedId = null;
      key.value = ""; list.replaceChildren(); detail.replaceChildren(); svg.replaceChildren();
      if (panel.parentNode === view) view.replaceChildren();
    },
  };
}
