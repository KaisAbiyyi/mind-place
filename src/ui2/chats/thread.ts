import type { AssetResult, Attachment, Message } from "../../contracts.js";
import { renderMessageContent, type Invoke } from "../../reader.js";
import type { Ui2I18nController } from "../i18n/index.js";
import { ALLOWED_IMAGE_MIME_TYPES, isSafeImageDataUrl } from "./assets.js";
import { displayMessageText } from "./display-text.js";
import { providerName } from "./format.js";

export interface ThreadDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
  /** The archive record the messages belong to (used only to fetch pictures). */
  archiveId: string;
  provider: string;
  /** False once the reader moved on or was destroyed: late picture loads are dropped. */
  isLive(): boolean;
}

function hiddenRole(doc: Document, label: string): HTMLElement {
  const span = doc.createElement("span");
  span.className = "ui2-chats-sr";
  span.textContent = label;
  return span;
}

async function loadPicture(deps: ThreadDeps, attachment: Attachment, slot: HTMLElement): Promise<void> {
  const { doc, i18n, invoke } = deps;
  try {
    const asset = await invoke<AssetResult>("get_asset", { id: deps.archiveId, assetPath: attachment.asset_path });
    if (!deps.isLive() || !slot.isConnected) return;
    const mimeType = asset.mime_type.toLowerCase();
    if (!ALLOWED_IMAGE_MIME_TYPES.has(mimeType) || !isSafeImageDataUrl(asset.data_url, mimeType)) {
      throw new Error("MP_UI2_ASSET_REJECTED");
    }
    const image = doc.createElement("img");
    image.className = "ui2-chats-image";
    image.alt = attachment.name || i18n.t("arch.imageAlt");
    image.decoding = "async";
    image.src = asset.data_url;
    slot.replaceChildren(image);
    slot.classList.remove("ui2-chats-slot-note");
  } catch {
    if (!deps.isLive() || !slot.isConnected) return;
    slot.textContent = i18n.t("arch.imageError");
  }
}

function renderAttachment(deps: ThreadDeps, attachment: Attachment): HTMLElement {
  const { doc, i18n } = deps;
  const item = doc.createElement("figure");
  item.className = "ui2-chats-attachment";
  const caption = doc.createElement("figcaption");
  caption.className = "ui2-chats-caption";
  if (attachment.mime_type.toLowerCase().startsWith("image/")) {
    const slot = doc.createElement("div");
    slot.className = "ui2-chats-slot ui2-chats-slot-note";
    if (attachment.asset_path) {
      slot.textContent = i18n.t("arch.imageLoading");
      void loadPicture(deps, attachment, slot);
    } else {
      slot.textContent = i18n.t("arch.imageUnavailable");
    }
    caption.textContent = attachment.name || i18n.t("arch.imageAlt");
    item.append(slot, caption);
    return item;
  }
  caption.textContent = attachment.name || i18n.t("arch.fileFallback");
  item.append(caption);
  return item;
}

/** One message of the thread: the owner's words on the right, everything else on the left. */
export function renderThreadMessage(deps: ThreadDeps, message: Message): HTMLElement {
  const { doc, i18n } = deps;
  const text = displayMessageText(message, deps.provider);
  const content = renderMessageContent(doc, { text, format: message.format });
  content.classList.add("ui2-chats-content");
  const article = doc.createElement("article");

  if (message.role === "user") {
    article.className = "ui2-chats-msg ui2-chats-user";
    article.append(hiddenRole(doc, i18n.t("archive.role.user")), content);
  } else if (message.role === "assistant") {
    article.className = "ui2-chats-msg ui2-chats-ai";
    const avatar = doc.createElement("span");
    avatar.className = "ui2-chats-avatar";
    avatar.setAttribute("aria-hidden", "true");
    avatar.textContent = providerName(deps.provider).slice(0, 1);
    const body = doc.createElement("div");
    body.className = "ui2-chats-ai-body";
    body.append(hiddenRole(doc, i18n.t("archive.role.assistant")), content);
    article.append(avatar, body);
  } else {
    // Tool and system steps: left aligned, with a small muted role caption, no bubble.
    article.className = "ui2-chats-msg ui2-chats-aux";
    const role = doc.createElement("p");
    role.className = "ui2-chats-role";
    role.textContent = i18n.t(message.role === "system" ? "arch.roleSystem" : "arch.roleTool");
    const body = doc.createElement("div");
    body.className = "ui2-chats-ai-body";
    body.append(content);
    article.append(role, body);
  }

  const host = article.querySelector<HTMLElement>(".ui2-chats-ai-body") ?? article;
  for (const attachment of message.attachments) host.append(renderAttachment(deps, attachment));
  return article;
}

/** The text copied for "Copy": the same words the reader shows, role by role. */
export function transcriptPlainText(
  i18n: Ui2I18nController,
  messages: readonly Message[],
  provider: string,
): string {
  return messages
    .map((message) => {
      const role = message.role === "user" ? i18n.t("archive.role.user")
        : message.role === "assistant" ? i18n.t("archive.role.assistant")
        : message.role === "system" ? i18n.t("arch.roleSystem")
        : i18n.t("arch.roleTool");
      return `${role}\n${displayMessageText(message, provider)}`;
    })
    .join("\n\n");
}
