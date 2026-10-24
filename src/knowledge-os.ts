/**
 * MP-8A: Knowledge OS UI contracts.
 *
 * A workspace is a read-only navigation concept, NOT a permission or data source.
 * All data still comes from live, unlock-epoch-gated native commands.
 */
export type WorkspaceId =
  | "conversations" | "memories" | "documents" | "connections"
  | "projects" | "people" | "topics" | "graph" | "health";

export interface WorkspaceDefinition {
  readonly id: WorkspaceId;
  readonly label: string;
  readonly action: string | null;
  readonly phase: "available" | "planned";
}

export const WORKSPACES: readonly WorkspaceDefinition[] = [
  { id: "conversations", label: "Conversations", action: "workspace-conversations", phase: "available" },
  { id: "memories", label: "Memories", action: "open-memory-inspector", phase: "available" },
  { id: "documents", label: "Documents", action: "open-document-search", phase: "available" },
  { id: "connections", label: "Connections", action: "open-setup", phase: "available" },
  { id: "projects", label: "Projects", action: "open-entity-workspace", phase: "available" },
  { id: "people", label: "People", action: "open-entity-workspace", phase: "available" },
  { id: "topics", label: "Topics", action: "open-entity-workspace", phase: "available" },
  { id: "graph", label: "Hive graph", action: null, phase: "planned" },
  { id: "health", label: "Health", action: null, phase: "planned" },
] as const;

export function createWorkspaceNavigation(document: Document): HTMLElement {
  const nav = document.createElement("nav");
  nav.id = "knowledge-os-nav";
  nav.className = "knowledge-os-nav";
  nav.setAttribute("aria-label", "Knowledge workspaces");
  // Planned workspaces are contracts, not clickable controls or false feature promises.
  for (const entry of WORKSPACES) {
    if (entry.phase !== "available" || !entry.action) continue;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "knowledge-os-nav-item";
    button.dataset.action = entry.action;
    button.dataset.workspace = entry.id;
    button.textContent = entry.label;
    nav.append(button);
  }
  setActiveWorkspace(nav, "conversations");
  return nav;
}

export function setActiveWorkspace(nav: HTMLElement, id: WorkspaceId): void {
  for (const button of nav.querySelectorAll<HTMLButtonElement>("[data-workspace]")) {
    if (button.dataset.workspace === id) button.setAttribute("aria-current", "page");
    else button.removeAttribute("aria-current");
  }
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
