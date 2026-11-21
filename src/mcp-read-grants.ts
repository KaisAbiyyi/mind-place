import type { Invoke } from "./reader.js";

/**
 * Owner-only read grants for local MCP clients (MP-7MCP-02 / MP-ADR-006).
 * A read credential discloses nothing until the owner grants it individual
 * archive sources. There is deliberately no wildcard or "all sources" call.
 * This module has no UI: screens call it once they exist (UI2 Connections).
 */
export const SOURCE_ID_PATTERN = /^src:v1:[0-9a-f]{64}$/;
/** Matches the native per-call limit; larger sets are sent in several calls. */
export const MAX_SOURCES_PER_REQUEST = 1000;

/** Validate, de-duplicate and keep order; throws on anything not a canonical id. */
export function canonicalSourceIds(ids: readonly string[]): string[] {
  const seen = new Set<string>();
  for (const id of ids) {
    if (typeof id !== "string" || !SOURCE_ID_PATTERN.test(id)) {
      throw new Error("MP_MCP_GRANT:Invalid source id");
    }
    seen.add(id);
  }
  return [...seen];
}

async function inChunks(
  invoke: Invoke,
  command: string,
  clientId: string,
  sourceIds: readonly string[],
): Promise<number> {
  const ids = canonicalSourceIds(sourceIds);
  let changed = 0;
  for (let at = 0; at < ids.length; at += MAX_SOURCES_PER_REQUEST) {
    changed += await invoke<number>(command, {
      clientId,
      sourceIds: ids.slice(at, at + MAX_SOURCES_PER_REQUEST),
    });
  }
  return changed;
}

/** Returns how many grants became newly active. */
export function grantMcpClientSources(
  invoke: Invoke,
  clientId: string,
  sourceIds: readonly string[],
): Promise<number> {
  return inChunks(invoke, "grant_local_mcp_client_sources", clientId, sourceIds);
}

export function revokeMcpClientSources(
  invoke: Invoke,
  clientId: string,
  sourceIds: readonly string[],
): Promise<number> {
  return inChunks(invoke, "revoke_local_mcp_client_sources", clientId, sourceIds);
}

export function revokeAllMcpClientSources(invoke: Invoke, clientId: string): Promise<number> {
  return invoke<number>("revoke_all_local_mcp_client_sources", { clientId });
}

/** Sorted canonical source ids the client may read (native cap: 10,000). */
export function listMcpClientSources(invoke: Invoke, clientId: string): Promise<string[]> {
  return invoke<string[]>("list_local_mcp_client_sources", { clientId });
}

/** One saved chat for the owner grant screen. `source_id` is opaque, never shown. */
export interface GrantableChat {
  source_id: string;
  title: string;
  provider: string;
  captured_at: string;
  granted: boolean;
}

export interface GrantableChats {
  /** Chats matching the query. */
  total: number;
  /** How many of `total` the client may currently read. */
  allowed: number;
  rows: GrantableChat[];
}

/** Native page cap; the screen asks for far fewer. */
export const MAX_GRANTABLE_PAGE = 100;

export function listGrantableChats(
  invoke: Invoke,
  clientId: string,
  options: { query?: string; offset?: number; limit?: number } = {},
): Promise<GrantableChats> {
  return invoke<GrantableChats>("list_local_mcp_grantable_chats", {
    clientId,
    query: options.query ?? null,
    offset: options.offset ?? 0,
    limit: Math.min(options.limit ?? 25, MAX_GRANTABLE_PAGE),
  });
}
