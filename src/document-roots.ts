import type { Invoke } from "./reader.js";

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
  heading.textContent = "Personal document folders — staging only";
  const explanation = doc.createElement("p");
  explanation.textContent =
    "Registering a folder only creates a pending identity. Mind Place will not read or index its files. Owner approval and protected NTFS access arrive in a later phase.";
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
