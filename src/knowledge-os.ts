import { UI_ROUTES, routeForWorkspace, type UiRouteId, type UiWorkspaceAlias } from "./ui-contracts.js";

export type WorkspaceId = UiWorkspaceAlias;
export interface WorkspaceDefinition {
  readonly id: WorkspaceId;
  readonly label: string;
  readonly action: string | null;
  readonly phase: "available" | "planned";
}
/** Legacy workspace ids remain internal aliases while visible navigation is route-first. */
export const WORKSPACES: readonly WorkspaceDefinition[] = [
  { id: "conversations", label: "Conversations", action: "workspace-conversations", phase: "available" },
  { id: "library", label: "Library search", action: "open-library-workspace", phase: "available" },
  { id: "memories", label: "Curated Memory", action: "open-memory-inspector", phase: "available" },
  { id: "documents", label: "Documents", action: "open-document-search", phase: "available" },
  { id: "connections", label: "Connections", action: "open-setup", phase: "available" },
  { id: "projects", label: "Projects", action: "open-entity-workspace", phase: "available" },
  { id: "people", label: "People", action: "open-entity-workspace", phase: "available" },
  { id: "topics", label: "Topics", action: "open-entity-workspace", phase: "available" },
  { id: "graph", label: "Hive Mind Graph", action: "open-hive-explorer", phase: "available" },
  { id: "health", label: "Diagnostics", action: "open-health-inspector", phase: "available" },
] as const;

const ROUTE_ICON_PATHS: Readonly<Record<UiRouteId, string>> = {
  overview: "M3 3h8v8H3z M13 3h8v5h-8z M13 10h8v11h-8z M3 13h8v8H3z",
  conversations: "M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v8a2.5 2.5 0 0 1-2.5 2.5H10l-5 4v-4.3A2.5 2.5 0 0 1 4 13.5z",
  "curated-memory": "M12 3v18 M4.5 7.5h15 M4.5 16.5h15 M7 3v4.5 M17 16.5V21",
  "hive-mind-graph": "M6 6l6 6 6-6 M6 18l6-6 6 6 M6 6h.01 M18 6h.01 M12 12h.01 M6 18h.01 M18 18h.01",
  "files-secure-vault": "M3 7.5A2.5 2.5 0 0 1 5.5 5H10l2 2h6.5A2.5 2.5 0 0 1 21 9.5v8a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z",
  "ai-providers-mcp": "M12 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6z M5 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6z M19 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6z M12 9v3 M7.5 15l3-3 M16.5 15l-3-3",
  "setup-onboarding": "M12 3v3 M12 18v3 M3 12h3 M18 12h3 M5.6 5.6l2.1 2.1 M16.3 16.3l2.1 2.1 M18.4 5.6l-2.1 2.1 M7.7 16.3l-2.1 2.1 M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z",
  settings: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z M4 12h2 M18 12h2 M12 4v2 M12 18v2 M6.3 6.3l1.4 1.4 M16.3 16.3l1.4 1.4 M17.7 6.3l-1.4 1.4 M7.7 16.3l-1.4 1.4",
};
function createRouteIcon(document: Document, route: UiRouteId): SVGSVGElement {
  const svg=document.createElementNS("http://www.w3.org/2000/svg","svg");
  svg.setAttribute("viewBox","0 0 24 24"); svg.setAttribute("aria-hidden","true"); svg.setAttribute("focusable","false");
  const path=document.createElementNS("http://www.w3.org/2000/svg","path");
  path.setAttribute("d",ROUTE_ICON_PATHS[route]); path.setAttribute("fill","none"); path.setAttribute("stroke","currentColor");
  path.setAttribute("stroke-width","1.6"); path.setAttribute("stroke-linecap","round"); path.setAttribute("stroke-linejoin","round");
  svg.append(path); return svg;
}
function createSecondaryButton(document: Document,label: string,workspace: WorkspaceId,action: string): HTMLButtonElement {
  const button=document.createElement("button"); button.type="button"; button.className="knowledge-os-nav-secondary";
  button.dataset.navItem="secondary"; button.dataset.navSecondary="true"; button.dataset.workspace=workspace; button.dataset.action=action;
  button.setAttribute("aria-pressed","false"); button.textContent=label; return button;
}
export function createWorkspaceNavigation(document: Document): HTMLElement {
  const nav=document.createElement("nav"); nav.id="knowledge-os-nav"; nav.className="knowledge-os-nav";
  nav.setAttribute("aria-label","Main navigation");
  const groups=[...new Set(UI_ROUTES.map((route)=>route.group))];
  for(const groupName of groups){
    const group=document.createElement("section"); group.className="knowledge-os-nav-group"; group.dataset.routeGroup=groupName;
    const heading=document.createElement("div"); heading.className="knowledge-os-nav-group-label"; heading.textContent=groupName; group.append(heading);
    for(const route of UI_ROUTES.filter((item)=>item.group===groupName)){
      const button=document.createElement("button"); button.type="button"; button.className="knowledge-os-nav-item";
      button.dataset.navItem="primary"; button.dataset.route=route.id; button.dataset.workspace=route.defaultWorkspace; button.dataset.action="navigate-route";
      button.title=route.label; button.setAttribute("aria-label",route.label);
      const icon=document.createElement("span"); icon.className="knowledge-os-nav-icon"; icon.append(createRouteIcon(document,route.id));
      const label=document.createElement("span"); label.className="nav-label"; label.textContent=route.label; button.append(icon,label); group.append(button);
      if(route.id==="overview"){
        const secondary=document.createElement("div"); secondary.className="knowledge-os-nav-secondary-group"; secondary.dataset.subnavFor=route.id;
        secondary.append(createSecondaryButton(document,"Search all sources","library","open-library-workspace")); group.append(secondary);
      }else if(route.id==="curated-memory"){
        const secondary=document.createElement("div"); secondary.className="knowledge-os-nav-secondary-group"; secondary.dataset.subnavFor=route.id;
        secondary.append(createSecondaryButton(document,"Projects","projects","open-entity-workspace"),createSecondaryButton(document,"People","people","open-entity-workspace"),createSecondaryButton(document,"Topics","topics","open-entity-workspace"));
        group.append(secondary);
      }else if(route.id==="settings"){
        const secondary=document.createElement("div"); secondary.className="knowledge-os-nav-secondary-group"; secondary.dataset.subnavFor=route.id;
        secondary.append(createSecondaryButton(document,"Diagnostics","health","open-health-inspector")); group.append(secondary);
      }
    }
    nav.append(group);
  }
  setActiveRoute(nav,"overview","conversations"); return nav;
}
export function setActiveRoute(nav: HTMLElement,route: UiRouteId,workspace: WorkspaceId = UI_ROUTES.find((item)=>item.id===route)!.defaultWorkspace): void {
  nav.dataset.activeRoute=route;
  for(const button of nav.querySelectorAll<HTMLButtonElement>("button[data-route]")){
    if(button.dataset.route===route) button.setAttribute("aria-current","page"); else button.removeAttribute("aria-current");
  }
  for(const button of nav.querySelectorAll<HTMLButtonElement>("button[data-nav-secondary]")){
    button.setAttribute("aria-pressed",String(button.dataset.workspace===workspace));
  }
}
export function setActiveWorkspace(nav: HTMLElement,id: WorkspaceId): void { setActiveRoute(nav,routeForWorkspace(id),id); }
export function bindWorkspaceNavigationKeyboard(nav: HTMLElement): () => void {
  const handler=(event: KeyboardEvent)=>{
    if(event.defaultPrevented||event.altKey||event.ctrlKey||event.metaKey)return;
    const target=event.target;
    if(!(target instanceof nav.ownerDocument.defaultView!.HTMLButtonElement)||!nav.contains(target))return;
    const buttons=[...nav.querySelectorAll<HTMLButtonElement>("button[data-nav-item]:not(:disabled)")];
    const index=buttons.indexOf(target); if(index<0)return;
    const next=event.key==="Home"?0:event.key==="End"?buttons.length-1:event.key==="ArrowDown"?Math.min(buttons.length-1,index+1):event.key==="ArrowUp"?Math.max(0,index-1):-1;
    if(next<0)return; event.preventDefault(); buttons[next]?.focus({preventScroll:true}); buttons[next]?.scrollIntoView?.({block:"nearest",inline:"nearest"});
  };
  nav.addEventListener("keydown",handler); return ()=>nav.removeEventListener("keydown",handler);
}
export const CONVERSATION_ROW_HEIGHT = 76;
export const VIRTUAL_LIST_THRESHOLD = 80;
export interface VirtualWindow {
  readonly start: number;
  readonly end: number;
  readonly beforePx: number;
  readonly afterPx: number;
}

/** Fixed-height O(visible rows) list window; offsets are clamped even on malformed input. */
export function getVirtualWindow(
  total: number,
  scrollTop: number,
  viewportHeight: number,
  rowHeight = CONVERSATION_ROW_HEIGHT,
  overscan = 6,
): VirtualWindow {
  const count = Number.isSafeInteger(total) && total > 0 ? total : 0;
  const height = Number.isFinite(rowHeight) && rowHeight > 0 ? rowHeight : CONVERSATION_ROW_HEIGHT;
  const visibleHeight = Number.isFinite(viewportHeight) && viewportHeight > 0 ? viewportHeight : 640;
  const extra = Number.isSafeInteger(overscan) && overscan >= 0 ? Math.min(overscan, 100) : 6;
  const maxScroll = Math.max(0, count * height - visibleHeight);
  const offset = Number.isFinite(scrollTop) ? Math.max(0, Math.min(scrollTop, maxScroll)) : 0;
  const first = Math.floor(offset / height);
  const start = Math.max(0, first - extra);
  const end = Math.min(count, Math.max(first + 1, Math.ceil((offset + visibleHeight) / height)) + extra);
  return { start, end, beforePx: start * height, afterPx: (count - end) * height };
}

/** Future workspace collection loader: no implicit reads, no authorizations, no shared cache. */
export interface KnowledgePage<T> {
  items: readonly T[];
  nextCursor: string | null;
}
export interface KnowledgePageRequest {
  cursor: string | null;
  limit: number;
  signal: AbortSignal;
}
export type KnowledgePageLoader<T> = (request: KnowledgePageRequest) => Promise<KnowledgePage<T>>;

export interface KnowledgeCollectionState<T> {
  status: "idle" | "loading" | "ready" | "error";
  items: readonly T[];
  nextCursor: string | null;
  error: string | null;
}

export function createKnowledgeCollection<T>(loader: KnowledgePageLoader<T>, pageSize = 50) {
  const limit = Number.isSafeInteger(pageSize) ? Math.max(1, Math.min(pageSize, 200)) : 50;
  let generation = 0;
  let controller: AbortController | null = null;
  let inFlight: Promise<void> | null = null;
  let state: KnowledgeCollectionState<T> = { status: "idle", items: [], nextCursor: null, error: null };
  let complete = false;

  const reset = (): void => {
    ++generation;
    controller?.abort();
    controller = null;
    inFlight = null;
    complete = false;
    state = { status: "idle", items: [], nextCursor: null, error: null };
  };
  const snapshot = (): KnowledgeCollectionState<T> => ({
    ...state, items: [...state.items],
  });
  const loadMore = (): Promise<void> => {
    if (inFlight) return inFlight;
    if (complete) return Promise.resolve();
    const expected = generation;
    const cursor = state.nextCursor;
    controller = new AbortController();
    const signal = controller.signal;
    state = { ...state, status: "loading", error: null };
    const task = (async () => {
      try {
        const page = await loader({ cursor, limit, signal });
        if (expected !== generation || signal.aborted) return;
        if (!page || !Array.isArray(page.items) ||
          (page.nextCursor !== null && typeof page.nextCursor !== "string") ||
          (page.nextCursor !== null && (page.nextCursor.length === 0 || page.nextCursor === cursor))) {
          throw new Error("MP_UI_INVALID_PAGE");
        }
        state = {
          status: "ready",
          items: [...state.items, ...page.items],
          nextCursor: page.nextCursor,
          error: null,
        };
        complete = page.nextCursor === null;
      } catch (error) {
        if (expected !== generation || signal.aborted) return;
        state = { ...state, status: "error", error: error instanceof Error ? error.message : "MP_UI_PAGE_FAILED" };
      }
    })();
    inFlight = task;
    void task.finally(() => {
      if (expected === generation) {
        inFlight = null;
        controller = null;
      }
    });
    return task;
  };
  return { snapshot, loadMore, reset };
}
