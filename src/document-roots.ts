import type { Invoke } from "./reader.js";

const REVOKE_CONFIRM_WINDOW_MS = 15_000;

export interface DocumentRoot {
  id: string;
  canonical_path: string;
  approval_state: "pending" | "approved" | "revoked";
  virtual_prefix: string;
}

/** Unlock-scoped OWNER setup, never mounted in the MCP/public read surface. */
export function mountDocumentRootSetup(parent: HTMLElement, invoke: Invoke): void {
  const doc = parent.ownerDocument;
  const section = doc.createElement("section");
  section.className = "document-root-setup";
  const heading = doc.createElement("h3");
  heading.className = "setup-section-title";
  heading.textContent = "Personal document folders — owner permissions";
  const explanation = doc.createElement("p");
  explanation.textContent =
    "Staging only records a pending identity. Approve explicitly to permit future bounded document reads. Revocation is permanent. Scanning and indexing are not enabled yet.";
  const form = doc.createElement("form");
  form.className = "document-root-form";
  const label = doc.createElement("label");
  label.textContent = "Existing folder (absolute path)";
  const input = doc.createElement("input");
  input.type = "text";
  input.name = "document-root-path";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.maxLength = 4096;
  input.placeholder = "C:\\Users\\…\\Documents";
  label.append(input);
  const stage = doc.createElement("button");
  stage.type = "submit";
  stage.className = "quiet-button";
  stage.textContent = "Stage folder";
  form.append(label, stage);
  const status = doc.createElement("p");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const list = doc.createElement("ul");
  list.className = "document-root-list";
  section.append(heading, explanation, form, status, list);
  parent.append(section);

  const refresh = async (): Promise<void> => {
    try {
      const roots = await invoke<DocumentRoot[]>("list_document_roots");
      if (!section.isConnected) return;
      list.replaceChildren();
      if (!roots.length) {
        const item = doc.createElement("li");
        item.textContent = "No document folders staged.";
        list.append(item);
      }
      for (const root of roots) {
        const item = doc.createElement("li");
        const name = doc.createElement("strong");
        name.textContent = root.canonical_path;
        const details = doc.createElement("small");
        details.textContent = ` ${root.approval_state.toUpperCase()} · ${root.virtual_prefix}`;
        item.append(name, details);
        if (root.approval_state === "pending") {
          const approve = doc.createElement("button");
          approve.type = "button";
          approve.className = "quiet-button";
          approve.dataset.action = "approve-root";
          approve.textContent = "Approve folder access";
          approve.addEventListener("click", () => {
            if (!section.isConnected) return;
            approve.disabled = true;
            status.textContent = "Checking folder identity and recording owner approval…";
            void invoke<DocumentRoot>("approve_document_root", { rootId: root.id })
              .then(async () => {
                if (!section.isConnected) return;
                status.textContent = "Approved. Scanning and indexing remain disabled.";
                await refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Approval denied: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) approve.disabled = false; });
          });
          item.append(approve);
        }
        if (root.approval_state !== "revoked") {
          const revoke = doc.createElement("button");
          revoke.type = "button";
          revoke.className = "quiet-button";
          revoke.dataset.action = "revoke-root";
          revoke.textContent = "Revoke";
          let armedAt: number | null = null;
          let resetTimer: ReturnType<typeof setTimeout> | undefined;
          const disarm = () => {
            armedAt = null;
            if (resetTimer !== undefined) clearTimeout(resetTimer);
            resetTimer = undefined;
            if (!revoke.disabled) revoke.textContent = "Revoke";
          };
          revoke.addEventListener("blur", () => {
            if (!revoke.disabled) disarm();
          });
          revoke.addEventListener("click", () => {
            if (!section.isConnected) return;
            const now = Date.now();
            if (armedAt === null || now - armedAt >= REVOKE_CONFIRM_WINDOW_MS) {
              disarm();
              armedAt = now;
              revoke.textContent = "Confirm permanent revoke (15s)";
              status.textContent = "Click again within 15 seconds to permanently revoke this folder.";
              resetTimer = setTimeout(() => {
                if (revoke.isConnected) disarm();
              }, REVOKE_CONFIRM_WINDOW_MS);
              return;
            }
            disarm();
            revoke.disabled = true;
            void invoke<void>("revoke_document_root", { rootId: root.id })
              .then(async () => {
                if (!section.isConnected) return;
                status.textContent = "Document root permanently revoked.";
                await refresh();
              })
              .catch((error: unknown) => {
                if (section.isConnected) status.textContent = `Revocation failed: ${String(error)}`;
              })
              .finally(() => { if (section.isConnected) revoke.disabled = false; });
          });
          item.append(revoke);
        }
        list.append(item);
      }
    } catch {
      if (section.isConnected) status.textContent = "Document registry unavailable while locked or offline.";
    }
  };

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const path = input.value.trim();
    if (!path || path.length > 4096) {
      status.textContent = "Enter an existing absolute folder path.";
      return;
    }
    stage.disabled = true;
    status.textContent = "Staging pending document identity…";
    void invoke<DocumentRoot>("stage_document_root", { path })
      .then(async (root) => {
        if (!section.isConnected) return;
        input.value = "";
        status.textContent = `Staged: ${root.approval_state}. No files were read or indexed.`;
        await refresh();
      })
      .catch((error: unknown) => {
        if (section.isConnected) status.textContent = `Could not stage folder: ${String(error)}`;
      })
      .finally(() => { if (section.isConnected) stage.disabled = false; });
  });
  void refresh();
}
