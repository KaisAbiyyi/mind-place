import type { Invoke } from "./reader.js";
import type { SecurityStatus } from "./security.js";
import { mountDocumentRootSetup } from "./document-roots.js";

export interface FilesVaultHandle { destroy(): void }

/**
 * Owner-only Files route. Never infer encryption for original documents from the
 * encrypted app vault, and never expose a path-scanning capability beyond root grants.
 */
export function mountFilesVaultWorkspace(view: HTMLElement, invoke: Invoke): FilesVaultHandle {
  const doc = view.ownerDocument;
  const shell = doc.createElement("section");
  shell.className = "files-vault-workspace";
  shell.setAttribute("aria-label", "Approved files and secure vault");
  const header = doc.createElement("header");
  header.className = "files-vault-heading";
  const heading = doc.createElement("h1"); heading.textContent = "Files & Secure Vault";
  const subtitle = doc.createElement("p");
  subtitle.textContent = "Your approved folders and verified indexed extracts. Original documents remain in their own locations.";
  header.append(heading, subtitle);
  const vault = doc.createElement("section"); vault.className = "files-vault-protection";
  vault.setAttribute("aria-label", "Vault protection status");
  const title = doc.createElement("h2"); title.textContent = "Storage protection";
  const protection = doc.createElement("p");
  protection.setAttribute("role", "status"); protection.setAttribute("aria-live", "polite");
  protection.textContent = "Checking native vault status…";
  const boundary = doc.createElement("p"); boundary.className = "files-vault-boundary";
  boundary.textContent = "Folder approval permits bounded indexing only after you choose Index. Revocation blocks future authorized reads and search access; it does not delete original files. Encrypted app-owned state is separate from external documents and backups.";
  const settings = doc.createElement("button"); settings.type = "button"; settings.className = "quiet-button";
  settings.dataset.action = "navigate-route"; settings.dataset.route = "settings";
  settings.textContent = "Security & recovery settings";
  vault.append(title, protection, boundary, settings);
  const roots = doc.createElement("div"); roots.className = "files-vault-roots";
  shell.append(header, vault, roots);
  view.replaceChildren(shell);
  mountDocumentRootSetup(roots, invoke);
  let alive = true;
  void invoke<SecurityStatus>("get_security_status").then(status => {
    if (!alive || !shell.isConnected) return;
    if (status.locked) {
      protection.textContent = "Vault locked. File permissions and index reads are unavailable.";
      roots.replaceChildren();
    } else if (status.vault_mode === "encrypted") {
      protection.textContent = "Native runtime reports encrypted app vault mode, unlocked for this session. This is not an encryption claim for original documents in approved folders.";
    } else if (status.vault_mode === "legacy") {
      protection.textContent = "Native runtime reports legacy security mode. Encryption of app-owned archive bytes is not verified for this profile.";
    } else {
      protection.textContent = "Native vault protection mode was not reported. Do not assume encrypted-at-rest protection.";
    }
  }).catch(() => {
    if (alive && shell.isConnected) protection.textContent = "Protection status unavailable. No encryption or unlock guarantee can be shown.";
  });
  return { destroy() { alive = false; roots.replaceChildren(); if (shell.parentNode === view) view.replaceChildren(); } };
}
