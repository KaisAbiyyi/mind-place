import type { Invoke } from "./reader.js";
import { mountSearchWorkspace, type ArchiveSearchHit } from "./search-workspace.js";

export type { ArchiveSearchHit };
export interface LibraryHandle {
  destroy(): void;
}

/**
 * Re-export and delegate to MP-UI-07 unified Search & Context workspace.
 */
export function mountLibraryWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void): LibraryHandle {
  const handle = mountSearchWorkspace(view, invoke, { onClose });
  return {
    destroy: () => handle.destroy(),
  };
}
