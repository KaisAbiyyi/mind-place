import { mountReader, type ClipboardWriter, type Invoke, type ReaderHandle } from "./reader.js";
import { mountSecurityGate, type SecurityStatus } from "./security.js";

interface ApplicationOptions {
  root: HTMLElement;
  invoke: Invoke;
  clipboard?: ClipboardWriter;
}

/** Own the reader's authorization lifetime; a disposed reader never serves a new epoch. */
export async function mountApplication({ root, invoke, clipboard }: ApplicationOptions): Promise<void> {
  let reader: ReaderHandle | undefined;
  let generation = 0;
  let locking = false;

  const showGate = async (requireLocked = false) => {
    const expectedGeneration = generation;
    const gateInvoke: Invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (generation !== expectedGeneration) throw new Error("MP_UI_SESSION_EXPIRED");
      const result = await invoke<T>(command, args);
      if (generation !== expectedGeneration) throw new Error("MP_UI_SESSION_EXPIRED");
      if (requireLocked && command === "get_security_status" && !(result as SecurityStatus).locked) {
        throw new Error("MP_UI_LOCK_UNCONFIRMED");
      }
      return result;
    };
    await mountSecurityGate({
      root,
      invoke: gateInvoke,
      onUnlocked: () => {
        if (generation !== expectedGeneration) return;
        generation++;
        const epoch = generation;
        const currentReader = mountReader({ root, invoke, clipboard, onLock: () => { void lock(); } });
        reader = currentReader;
        // The first library read and the derived index would otherwise scan
        // the same thousands of archive files concurrently on cold unlock.
        // Let the owner see the library before starting the background index.
        // A late list completion must never start indexing after Lock.
        void currentReader.ready.then(() => {
          if (generation !== epoch || reader !== currentReader) return;
          return invoke("refresh_archive_index");
        }).catch(() => {});
      },
    });
  };

  const lock = async (): Promise<void> => {
    if (locking) return;
    locking = true;
    generation++;
    reader?.destroy();
    reader = undefined;
    root.replaceChildren();
    const status = root.ownerDocument.createElement("p");
    status.setAttribute("role", "status");
    status.textContent = "Locking Mind Place…";
    root.append(status);
    try {
      const result = await invoke<SecurityStatus>("lock_security");
      if (!result.locked) throw new Error("MP_UI_LOCK_UNCONFIRMED");
      await showGate(true);
    } catch {
      // Hiding the reader is not proof of native lock; no unlock form until confirmed.
      root.innerHTML = '<main class="security-gate"><section class="security-card">' +
        '<h1>Lock not confirmed</h1><p role="alert">Mind Place could not confirm the lock. Services may still be running. Retry Lock or quit Mind Place.</p>' +
        '<button type="button" class="primary-button" data-action="retry-lock">Retry Lock</button></section></main>';
      const retry = root.querySelector<HTMLButtonElement>("[data-action='retry-lock']")!;
      retry.addEventListener("click", () => { void lock(); });
      retry.focus();
    } finally {
      locking = false;
    }
  };

  await showGate();
}
