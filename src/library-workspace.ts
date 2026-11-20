import type { Invoke, ClipboardWriter } from "./reader.js";
import { mountSearchWorkspace, type ArchiveSearchHit } from "./search-workspace.js";

export type { ArchiveSearchHit };
export interface LibraryHandle {
  destroy(): void;
}

/**
 * Re-export and delegate to MP-UI-07 unified Search & Context workspace.
 */
export function mountLibraryWorkspace(view: HTMLElement, invoke: Invoke, onClose: () => void, clipboard?: ClipboardWriter, onSelectConversation?: (id: string, snapshotId?: string) => void): LibraryHandle {
  const handle = mountSearchWorkspace(view, invoke, { onClose, clipboard, onSelectConversation });
  return {
    destroy: () => handle.destroy(),
  };
}
