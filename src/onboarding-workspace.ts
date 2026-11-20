import type { Invoke } from "./reader.js";
import type { Conversation, ConversationSummary, SetupInfo } from "./contracts.js";
import type { SecurityStatus } from "./security.js";
import type { ArchiveSearchHit } from "./library-workspace.js";
import type { UiModuleDataState, UiModuleError, UiModuleHandle, UiRouteId } from "./ui-contracts.js";
import { createUiButton, createUiCard, createUiStatePanel, createUiStatus, type UiStatusState } from "./ui-primitives.js";

/**
 * MP-8ONB-01 — guided setup wizard (HTML `page-setup`).
 *
 * Every status shown here comes from a real backend command or is labelled as
 * unverified. The wizard never asks for, receives, or stores the unlock secret
 * or the offline recovery code (the lock screen owns them), and progress storage
 * holds only the step number and skip/finish markers. The security gate remains
 * authoritative: this module cannot grant or imply authorization.
 */
export type OnboardingStepId = "welcome" | "vault" | "providers" | "first-result";
export interface OnboardingStepDefinition { readonly id: OnboardingStepId; readonly label: string; readonly requirement: "Required" | "Optional"; }
export const ONBOARDING_STEPS: readonly OnboardingStepDefinition[] = [
  { id: "welcome", label: "Welcome & architecture", requirement: "Optional" },
  { id: "vault", label: "Secure vault", requirement: "Required" },
  { id: "providers", label: "Capture & AI providers", requirement: "Optional" },
  { id: "first-result", label: "First useful result", requirement: "Required" },
] as const;

export interface OnboardingProgress {
  readonly version: 1;
  readonly step: OnboardingStepId;
  readonly providersSkipped: boolean;
  readonly finishedAt: string | null;
}
export interface OnboardingProgressStore {
  read(): OnboardingProgress | null;
  write(progress: OnboardingProgress): void;
}
export interface OnboardingHooks {
  openRoute(route: UiRouteId, trigger: HTMLElement): void;
  openSetupDetails(trigger: HTMLElement): void;
  openConversation(id: string, snapshotId?: string): void;
  onFinish(): void;
}
export interface OnboardingSnapshot {
  readonly step: OnboardingStepId;
  readonly providersSkipped: boolean;
  readonly firstResultVerified: boolean;
}

export const ONBOARDING_STORAGE_KEY = "mind-place.onboarding.v1";

const STEP_IDS = new Set<string>(ONBOARDING_STEPS.map((step) => step.id));
export function parseOnboardingProgress(raw: unknown): OnboardingProgress | null {
  if (!raw || typeof raw !== "object") return null;
  const value = raw as Record<string, unknown>;
  if (value.version !== 1 || typeof value.step !== "string" || !STEP_IDS.has(value.step)) return null;
  if (typeof value.providersSkipped !== "boolean") return null;
  const finishedAt = value.finishedAt;
  if (finishedAt !== null && (typeof finishedAt !== "string" || Number.isNaN(Date.parse(finishedAt)))) return null;
  return { version: 1, step: value.step as OnboardingStepId, providersSkipped: value.providersSkipped, finishedAt };
}
/** Non-sensitive progress only. Storage failure degrades to in-memory progress. */
export function createBrowserProgressStore(win: Window | null | undefined): OnboardingProgressStore {
  return {
    read() {
      try {
        const text = win?.localStorage.getItem(ONBOARDING_STORAGE_KEY);
        return text ? parseOnboardingProgress(JSON.parse(text)) : null;
      } catch { return null; }
    },
    write(progress) {
      try { win?.localStorage.setItem(ONBOARDING_STORAGE_KEY, JSON.stringify(progress)); } catch { /* progress is best-effort */ }
    },
  };
}
export function createMemoryProgressStore(initial: OnboardingProgress | null = null): OnboardingProgressStore {
  let current = initial;
  return { read: () => current, write: (progress) => { current = progress; } };
}
export function hasFinishedOnboarding(store: OnboardingProgressStore): boolean {
  return store.read()?.finishedAt != null;
}

type Section<T> =
  | { readonly status: "pending" }
  | { readonly status: "ready"; readonly value: T }
  | { readonly status: "error"; readonly error: UiModuleError; readonly locked: boolean };
const PENDING = { status: "pending" } as const;

export interface StepStatus { readonly state: UiStatusState; readonly label: string; }

export function isLockedFailure(error: unknown): boolean {
  const text = error instanceof Error ? error.message : String(error ?? "");
  return /LOCKED|NOT_UNLOCKED|UNLOCK_REQUIRED|SESSION_EXPIRED|EPOCH|NOT_AUTHORIZED/i.test(text);
}
function toSectionError(error: unknown, subject: string): { status: "error"; error: UiModuleError; locked: boolean } {
  const locked = isLockedFailure(error);
  return {
    status: "error", locked,
    error: { code: locked ? "MP_UI_LOCKED" : "MP_UI_UNAVAILABLE", message: locked ? "Mind Place is locked." : subject + " is unavailable.", retryable: true },
  };
}

export function describeVault(section: Section<SecurityStatus>): StepStatus {
  if (section.status === "pending") return { state: "loading", label: "Checking vault…" };
  if (section.status === "error") return section.locked ? { state: "locked", label: "Locked" } : { state: "error", label: "Status unavailable" };
  const status = section.value;
  if (status.locked) return { state: "locked", label: "Locked" };
  if (!status.configured) return { state: "partial", label: "Not set up yet" };
  return status.vault_mode === "encrypted"
    ? { state: "complete", label: "Encrypted vault unlocked" }
    : { state: "partial", label: "Unlock secret only (archive not encrypted)" };
}
export function describeCapture(section: Section<SetupInfo>): StepStatus {
  if (section.status === "pending") return { state: "loading", label: "Checking capture setup…" };
  if (section.status === "error") return { state: section.locked ? "locked" : "error", label: section.locked ? "Locked" : "Status unavailable" };
  const count = section.value.registered_browsers.length;
  // Registration writes native-host configuration only; it is not a tested connection.
  return count > 0
    ? { state: "partial", label: count === 1 ? "1 browser registered (connection untested)" : count + " browsers registered (connection untested)" }
    : { state: "neutral", label: "Not registered" };
}
export function describeMcp(section: Section<{ running: boolean }>): StepStatus {
  if (section.status === "pending") return { state: "loading", label: "Checking local MCP…" };
  if (section.status === "error") return { state: section.locked ? "locked" : "error", label: section.locked ? "Locked" : "Status unavailable" };
  // A listener being alive does not prove any AI client has connected.
  return section.value.running ? { state: "partial", label: "Listener running (no client verified)" } : { state: "neutral", label: "Stopped" };
}

function newestSummary(list: readonly ConversationSummary[]): ConversationSummary | null {
  let best: ConversationSummary | null = null;
  let bestTime = Number.NEGATIVE_INFINITY;
  for (const item of list) {
    const time = Date.parse(item.captured_at);
    const rank = Number.isNaN(time) ? Number.NEGATIVE_INFINITY : time;
    if (best === null || rank > bestTime) { best = item; bestTime = rank; }
  }
  return best;
}
function titleCase(value: string): string { return value ? value[0].toUpperCase() + value.slice(1) : value; }
function formatCapturedAt(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "date unavailable" : date.toISOString().slice(0, 16).replace("T", " ") + " UTC";
}

interface ArchiveOverview { readonly count: number; readonly latest: ConversationSummary | null; }
interface Notice { readonly text: string; readonly error: boolean; }
interface VerifiedResult { readonly title: string; readonly archiveId: string; readonly snapshotId: string; readonly provider: string; readonly capturedAt: string; readonly excerpt: string; readonly via: "latest" | "search"; }
interface ResultProblem { readonly title: string; readonly detail: string; readonly state: "error" | "locked" | "empty"; }

export interface OnboardingWorkspaceOptions {
  readonly view: HTMLElement;
  readonly invoke: Invoke;
  readonly hooks: OnboardingHooks;
  readonly store: OnboardingProgressStore;
  readonly now?: () => Date;
}

export interface OnboardingWorkspaceHandle extends UiModuleHandle {
  readonly element: HTMLElement;
  goTo(step: OnboardingStepId): void;
}

export function mountOnboardingWorkspace(options: OnboardingWorkspaceOptions): OnboardingWorkspaceHandle {
  const { view, invoke, hooks, store } = options;
  const doc = view.ownerDocument;
  const now = options.now ?? (() => new Date());
  let alive = true;
  let refreshVersion = 0;
  let actionVersion = 0;
  const saved = store.read();
  let step: OnboardingStepId = saved && saved.finishedAt === null ? saved.step : "welcome";
  let providersSkipped = saved?.providersSkipped ?? false;
  let finishedAt: string | null = saved?.finishedAt ?? null;
  let security: Section<SecurityStatus> = PENDING;
  let setup: Section<SetupInfo> = PENDING;
  let mcp: Section<{ running: boolean }> = PENDING;
  let archive: Section<ArchiveOverview> = PENDING;
  let notice: Notice | null = null;
  let verified: VerifiedResult | null = null;
  let problem: ResultProblem | null = null;
  let hits: ArchiveSearchHit[] | null = null;
  let query = "";
  let busy = new Set<string>();

  const root = doc.createElement("section");
  root.className = "onboarding-workspace";
  root.setAttribute("aria-label", "Setup and onboarding");
  const header = doc.createElement("header");
  header.className = "onboarding-heading";
  const headingText = doc.createElement("div");
  const eyebrow = doc.createElement("p"); eyebrow.className = "onboarding-eyebrow"; eyebrow.textContent = "FIRST-RUN SETUP";
  const title = doc.createElement("h1"); title.textContent = "Set up Mind Place"; title.tabIndex = -1;
  const sub = doc.createElement("p"); sub.className = "onboarding-sub";
  headingText.append(eyebrow, title, sub);
  const counter = doc.createElement("span"); counter.className = "onboarding-counter";
  header.append(headingText, counter);
  const layout = doc.createElement("div"); layout.className = "onboarding-layout";
  const stepper = doc.createElement("nav"); stepper.className = "onboarding-steps"; stepper.setAttribute("aria-label", "Setup steps");
  const stepList = doc.createElement("ol");
  stepper.append(stepList);
  const panel = doc.createElement("div"); panel.className = "onboarding-panel";
  layout.append(stepper, panel);
  root.append(header, layout);
  view.replaceChildren(root);
  view.setAttribute("aria-busy", "false");

  const persist = () => {
    if (!alive) return;
    store.write({ version: 1, step, providersSkipped, finishedAt });
  };
  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, text?: string, className?: string): HTMLElementTagNameMap[K] => {
    const element = doc.createElement(tag);
    if (text !== undefined) element.textContent = text;
    if (className) element.className = className;
    return element;
  };
  const stepIndex = (id: OnboardingStepId) => ONBOARDING_STEPS.findIndex((item) => item.id === id);
  const action = (label: string, key: string, onClick: (button: HTMLButtonElement) => void, variant: "primary" | "secondary" | "quiet" = "secondary", disabled = false) => {
    const button = createUiButton(doc, label, { variant, disabled: disabled || busy.has(key) });
    button.dataset.focusKey = key;
    button.addEventListener("click", () => { if (alive) onClick(button); });
    return button;
  };
  const withBusy = async (key: string, work: () => Promise<void>) => {
    if (busy.has(key)) return;
    busy = new Set(busy).add(key);
    render();
    try { await work(); } finally {
      if (alive) { busy = new Set([...busy].filter((item) => item !== key)); render(); }
    }
  };
  const field = (label: string, value: string, mono = false) => {
    const row = make("div", undefined, "onboarding-fact");
    const term = make("dt", label);
    const detail = make("dd");
    if (mono) detail.append(make("code", value)); else detail.textContent = value;
    row.append(term, detail);
    return row;
  };
  const noticeLine = (scope: string) => {
    const line = make("p", notice?.text ?? "", "onboarding-notice");
    line.setAttribute("role", notice?.error ? "alert" : "status");
    line.setAttribute("aria-live", notice?.error ? "assertive" : "polite");
    line.dataset.scope = scope;
    if (!notice) line.hidden = true;
    return line;
  };
  const navigation = (back: boolean, next: { label: string; key: string } | null, extra?: HTMLElement[]) => {
    const row = make("div", undefined, "onboarding-actions");
    const left = make("div", undefined, "onboarding-actions-start");
    const right = make("div", undefined, "onboarding-actions-end");
    if (back) left.append(action("Back", "back", () => goTo(ONBOARDING_STEPS[Math.max(0, stepIndex(step) - 1)].id)));
    if (extra) right.append(...extra);
    if (next) right.append(action(next.label, next.key, () => goTo(ONBOARDING_STEPS[Math.min(ONBOARDING_STEPS.length - 1, stepIndex(step) + 1)].id), "primary"));
    row.append(left, right);
    return row;
  };

  // ---- Data -------------------------------------------------------------
  const failure = (error: unknown, subject: string) => toSectionError(error, subject);
  const loadSecurity = async (version: number) => {
    try {
      const value = await invoke<SecurityStatus>("get_security_status");
      if (alive && version === refreshVersion) security = { status: "ready", value };
    } catch (error) { if (alive && version === refreshVersion) security = failure(error, "Vault status"); }
  };
  const loadSetup = async (version: number) => {
    try {
      const value = await invoke<SetupInfo>("get_setup");
      if (alive && version === refreshVersion) setup = { status: "ready", value };
    } catch (error) { if (alive && version === refreshVersion) setup = failure(error, "Capture setup"); }
  };
  const loadMcp = async (version: number) => {
    try {
      const value = await invoke<{ running: boolean }>("get_local_mcp_status");
      if (alive && version === refreshVersion) mcp = { status: "ready", value: { running: value.running === true } };
    } catch (error) { if (alive && version === refreshVersion) mcp = failure(error, "Local MCP status"); }
  };
  const loadArchive = async (version: number) => {
    try {
      const list = await invoke<ConversationSummary[]>("list_conversations");
      if (!Array.isArray(list)) throw new Error("MP_UI_INVALID_RESPONSE");
      if (alive && version === refreshVersion) archive = { status: "ready", value: { count: list.length, latest: newestSummary(list) } };
    } catch (error) { if (alive && version === refreshVersion) archive = failure(error, "Archive"); }
  };
  const loadFor = async (id: OnboardingStepId, version: number) => {
    // Load only what the visible step explains. The archive list is read
    // only on the final step so cold unlock stays cheap on large archives.
    if (id === "vault") await loadSecurity(version);
    else if (id === "providers") await Promise.all([loadSetup(version), loadMcp(version)]);
    else if (id === "first-result") await loadArchive(version);
  };
  const dataLoad = async (): Promise<void> => {
    const version = ++refreshVersion;
    const current = step;
    if (current === "vault") security = PENDING;
    else if (current === "providers") { setup = PENDING; mcp = PENDING; }
    else if (current === "first-result") { archive = PENDING; }
    render();
    await loadFor(current, version);
    if (alive && version === refreshVersion) render();
  };

  // ---- Step changes -----------------------------------------------------
  function goTo(next: OnboardingStepId, moveFocus = true): void {
    if (!alive) return;
    step = next;
    notice = null;
    problem = null;
    persist();
    ++actionVersion;
    void dataLoad();
    if (moveFocus) panel.querySelector<HTMLElement>("h2")?.focus({ preventScroll: true });
  }

  // ---- Actions ----------------------------------------------------------
  const registerBrowser = (browser: "chrome" | "edge" | "brave") => withBusy("register-" + browser, async () => {
    const version = ++actionVersion;
    notice = { text: "Registering " + titleCase(browser) + "…", error: false };
    render();
    try {
      const info = await invoke<SetupInfo>("register_extension", { browser });
      if (!alive || version !== actionVersion) return;
      setup = { status: "ready", value: info };
      notice = { text: titleCase(browser) + " native-host configuration saved; the browser connection has not been tested.", error: false };
    } catch (error) {
      if (!alive || version !== actionVersion) return;
      notice = { text: isLockedFailure(error) ? "Mind Place is locked. Unlock it before registering a browser." : "Couldn’t register " + titleCase(browser) + ". Nothing was changed; try again or open full capture settings.", error: true };
    }
  });
  const skipProviders = () => {
    providersSkipped = true;
    goTo("first-result");
  };
  const verifyRead = async (summaryId: string, snapshotId: string | undefined, via: "latest" | "search", messageId?: string) => {
    const version = ++actionVersion;
    verified = null; problem = null;
    notice = { text: "Reading the saved conversation…", error: false };
    render();
    try {
      const result = await invoke<Conversation>("read_conversation", snapshotId ? { id: summaryId, snapshotId } : { id: summaryId });
      if (!alive || version !== actionVersion) return;
      if (result.summary.id !== summaryId || (snapshotId && result.snapshot_id && result.snapshot_id !== snapshotId)) {
        throw new Error("MP_UI_SOURCE_MISMATCH");
      }
      const messages = result.capture.messages;
      if (!Array.isArray(messages) || messages.length === 0) {
        problem = { state: "error", title: "The conversation has no readable messages", detail: "Mind Place read this capture but it contains no messages, so it cannot be used as a first result. Capture it again from the provider page." };
        notice = null;
        return;
      }
      const target = messageId && messageId !== "@title" ? messages.find((message) => message.id === messageId) : messages[0];
      const excerptSource = target?.text ?? messages[0].text;
      verified = {
        title: result.summary.title || "Untitled conversation", archiveId: summaryId,
        snapshotId: result.snapshot_id ?? "latest revision", provider: result.summary.provider,
        capturedAt: result.capture.captured_at, via,
        excerpt: excerptSource.slice(0, 600) + (excerptSource.length > 600 ? "…" : ""),
      };
      notice = null;
    } catch (error) {
      if (!alive || version !== actionVersion) return;
      notice = null;
      problem = isLockedFailure(error)
        ? { state: "locked", title: "Mind Place is locked", detail: "The read was refused because the vault is locked or the session ended. Unlock Mind Place from the lock screen, then try again." }
        : { state: "error", title: "Couldn’t read that conversation", detail: "The saved conversation could not be read or no longer matches the selected revision. Try again, or choose a different result." };
    }
  };
  const runSearch = () => withBusy("search", async () => {
    const text = query.trim();
    const version = ++actionVersion;
    hits = null; verified = null; problem = null;
    if (!text || text.length > 256) { notice = { text: "Enter a search phrase of 1–256 characters.", error: true }; return; }
    notice = { text: "Searching saved conversations…", error: false };
    render();
    try {
      const found = await invoke<ArchiveSearchHit[]>("search_archive_index", { query: text, includeHistory: true, limit: 5 });
      if (!alive || version !== actionVersion) return;
      hits = Array.isArray(found) ? found.slice(0, 5) : [];
      notice = { text: hits.length ? hits.length + " match" + (hits.length === 1 ? "" : "es") + " found. Choose one to read its source." : "No matches yet. Try another word, or read your latest conversation instead.", error: false };
    } catch (error) {
      if (!alive || version !== actionVersion) return;
      notice = null;
      problem = isLockedFailure(error)
        ? { state: "locked", title: "Mind Place is locked", detail: "Search is unavailable while the vault is locked. Unlock Mind Place from the lock screen, then try again." }
        : { state: "error", title: "Search is unavailable", detail: "The search index could not be queried. Your archive was not changed. Retry, or read your latest conversation instead." };
    }
  });

  // ---- Rendering --------------------------------------------------------
  function sectionPanel(state: UiStatusState, heading: string, detail: string): HTMLElement {
    return createUiStatePanel(doc, { state: state === "locked" ? "locked" : state === "error" ? "error" : "loading", title: heading, detail });
  }
  function statusRow(label: string, status: StepStatus): HTMLElement {
    const row = make("div", undefined, "onboarding-status-row");
    row.append(make("span", label), createUiStatus(doc, status.label, status.state));
    return row;
  }
  function stepHeading(overline: string, heading: string, lead: string): HTMLElement[] {
    const h2 = make("h2", heading); h2.tabIndex = -1; h2.dataset.focusKey = "heading";
    return [make("p", overline, "onboarding-eyebrow"), h2, make("p", lead, "onboarding-lead")];
  }

  function renderWelcome(): void {
    const cards = make("div", undefined, "onboarding-cards");
    for (const [name, text] of [
      ["Local vault", "Your archive stays on this device. Mind Place starts locked and opens only with the secret you choose."],
      ["Read-only AI access", "AI tools can reach your archive only through a local, read-only endpoint that you start after unlocking. Nothing is shared by default."],
      ["Traceable results", "Every result points back to the exact saved conversation revision it came from."],
    ] as const) {
      const card = createUiCard(doc, { title: name, description: text });
      card.element.classList.add("onboarding-card");
      cards.append(card.element);
    }
    const plan = createUiCard(doc, { title: "What is required and what is optional", description: "You can finish without optional steps and return to this page at any time." });
    const list = make("ul", undefined, "onboarding-list");
    for (const text of [
      "Required — your unlock secret and vault, created on the lock screen.",
      "Required — one real read of your own saved conversation to confirm everything works.",
      "Optional — browser capture, AI provider connections and document folders.",
    ]) list.append(make("li", text));
    plan.body.append(list);
    panel.append(...stepHeading("STEP 1 · ARCHITECTURE OVERVIEW", "Welcome to Mind Place", "Mind Place gives your saved AI conversations and approved documents one private, local-first home with their sources intact."), cards, plan.element, navigation(false, { label: "Check my vault", key: "next" }));
  }

  function renderVault(): void {
    const vault = describeVault(security);
    panel.append(...stepHeading("STEP 2 · SECURITY", "Check your secure vault", "Your unlock secret is entered only on the lock screen. This page never asks for, stores or displays it."));
    if (security.status === "pending") {
      panel.append(sectionPanel("loading", "Checking vault status", "Reading the current lock and protection state."));
    } else if (security.status === "error") {
      panel.append(sectionPanel(security.locked ? "locked" : "error", security.locked ? "Mind Place is locked" : "Vault status is unavailable", security.locked ? "Unlock Mind Place from the lock screen, then check again." : "The vault state could not be read. Nothing was changed. Check again, or restart Mind Place."));
    } else {
      const card = createUiCard(doc, { title: "Vault status", description: "Read from the local security service just now." });
      const facts = make("dl", undefined, "onboarding-facts");
      const status = security.value;
      facts.append(
        field("Lock state", status.locked ? "Locked" : "Unlocked for this session"),
        field("Protection", status.vault_mode === "encrypted" ? "Encrypted vault" : "Unlock secret only — existing archive files are not encrypted at rest"),
        field("Setup", status.configured ? "Unlock secret created" : "No unlock secret yet — create it on the lock screen"),
      );
      card.body.append(statusRow("Vault", vault), facts);
      if (status.vault_mode === "encrypted") card.body.append(make("p", "Keep your offline recovery code somewhere private outside Mind Place. Without it, a Windows account or device change can prevent recovery.", "onboarding-note"));
      panel.append(card.element);
    }
    panel.append(noticeLine("vault"), navigation(true, { label: "Continue to providers", key: "next" }, [action("Check again", "refresh-vault", () => { notice = null; void dataLoad(); }, "quiet")]));
  }

  function renderProviders(): void {
    panel.append(...stepHeading("STEP 3 · OPTIONAL", "Connect capture and AI providers", "Nothing here is mandatory. Each item explains the access it needs before you grant it."));
    if (providersSkipped) {
      const skipped = make("p", "You skipped this step earlier. You can set these up now or come back from Setup & Onboarding at any time.", "onboarding-note");
      skipped.dataset.skipped = "true";
      panel.append(skipped);
    }
    const capture = createUiCard(doc, { title: "Browser capture", description: "Save ChatGPT, Claude or Gemini conversations from the page you are viewing." });
    capture.body.append(statusRow("Native host", describeCapture(setup)));
    const permissions = make("ul", undefined, "onboarding-list");
    for (const text of [
      "activeTab + scripting — read only the conversation already loaded on the page you choose to save.",
      "nativeMessaging — hand the saved capture to Mind Place on this computer.",
      "storage — remember extension preferences.",
      "Automatic saving is off unless you enable it. It is limited to chatgpt.com, reads only what is already loaded and never scrolls.",
    ]) permissions.append(make("li", text));
    capture.body.append(permissions);
    if (setup.status === "ready") {
      const info = setup.value;
      const facts = make("dl", undefined, "onboarding-facts");
      facts.append(field("Extension folder", info.extension_path || "Unavailable", true));
      capture.body.append(make("p", "In your browser’s extensions page, enable developer mode, choose Load unpacked, then select the extension folder below. Register the browser you use so manual saves reach this archive.", "onboarding-note"), facts);
      const buttons = make("div", undefined, "onboarding-button-row");
      for (const browser of ["chrome", "edge", "brave"] as const) {
        const registered = info.registered_browsers.map((item) => item.toLowerCase()).includes(browser);
        buttons.append(action((registered ? "Re-register " : "Register ") + titleCase(browser), "register-" + browser, () => { void registerBrowser(browser); }, "secondary"));
      }
      capture.body.append(buttons);
    } else if (setup.status === "error") {
      capture.body.append(sectionPanel(setup.locked ? "locked" : "error", setup.locked ? "Mind Place is locked" : "Capture setup is unavailable", setup.locked ? "Unlock Mind Place, then check again." : "Capture details could not be read. Check again or open full capture settings."));
    }
    const providers = createUiCard(doc, { title: "AI provider connections (MCP)", description: "Let a local AI client read your archive. Claude and Gemini capture above does not connect them as providers." });
    providers.body.append(statusRow("Local MCP", describeMcp(mcp)), make("p", "Access is read-only and loopback-only, and starts only when you start it after unlocking. A running listener does not prove that any AI client has connected.", "onboarding-note"));
    providers.body.append(action("Open AI Providers & MCP", "open-providers", (button) => hooks.openRoute("ai-providers-mcp", button)));
    const files = createUiCard(doc, { title: "Document folders", description: "Search files you approve. Nothing is scanned automatically." });
    files.body.append(action("Open Files & Secure Vault", "open-files", (button) => hooks.openRoute("files-secure-vault", button)));
    const grid = make("div", undefined, "onboarding-stack");
    grid.append(capture.element, providers.element, files.element);
    panel.append(grid, noticeLine("providers"));
    panel.append(navigation(true, { label: "Continue to first result", key: "next" }, [
      action("Open full capture settings", "open-details", (button) => hooks.openSetupDetails(button), "quiet"),
      action("Skip for now", "skip-providers", skipProviders, "quiet"),
      action("Refresh status", "refresh-providers", () => { notice = null; void dataLoad(); }, "quiet"),
    ]));
  }

  function renderFirstResult(): void {
    panel.append(...stepHeading("STEP 4 · FIRST RESULT", "Read your first saved conversation", "Success means Mind Place actually found and read something from your archive — not a demo."));
    if (archive.status === "pending") {
      panel.append(sectionPanel("loading", "Checking your archive", "Looking for saved conversations."));
      panel.append(navigation(true, null));
      return;
    }
    if (archive.status === "error") {
      panel.append(sectionPanel(archive.locked ? "locked" : "error", archive.locked ? "Mind Place is locked" : "Your archive couldn’t be read", archive.locked ? "Unlock Mind Place from the lock screen, then check again." : "The archive list could not be loaded. Nothing was changed. Check again."));
      panel.append(navigation(true, null, [action("Check again", "refresh-archive", () => { notice = null; void dataLoad(); }, "primary")]));
      return;
    }
    const overview = archive.value;
    if (overview.count === 0) {
      const empty = createUiStatePanel(doc, { state: "empty", title: "No saved conversations yet", detail: "Your vault is ready, but the archive is empty. Save a conversation with the browser extension (open ChatGPT, Claude or Gemini, then use the Mind Place button) or import existing files, then check again." });
      empty.classList.add("onboarding-empty");
      panel.append(empty);
      panel.append(navigation(true, null, [
        action("Import or set up capture", "open-details", (button) => hooks.openSetupDetails(button)),
        action("Check again", "refresh-archive", () => { notice = null; void dataLoad(); }, "primary"),
      ]));
      panel.append(finishRow(false));
      return;
    }
    const intro = createUiCard(doc, { title: overview.count + " saved conversation" + (overview.count === 1 ? "" : "s"), description: "Search for a word you remember, or read the most recent capture directly." });
    const form = make("form", undefined, "onboarding-search");
    const label = make("label", "Search phrase"); label.htmlFor = "onboarding-query";
    const input = make("input"); input.type = "search"; input.id = "onboarding-query"; input.maxLength = 256; input.autocomplete = "off"; input.value = query;
    input.dataset.focusKey = "query";
    input.addEventListener("input", () => { query = input.value; });
    const submit = createUiButton(doc, "Search", { variant: "primary", type: "submit", disabled: busy.has("search") });
    submit.dataset.focusKey = "search";
    form.addEventListener("submit", (event) => { event.preventDefault(); if (alive) void runSearch(); });
    form.append(label, input, submit);
    intro.body.append(form);
    if (overview.latest) {
      const latest = overview.latest;
      intro.body.append(action("Read latest conversation (" + (latest.title || "Untitled") + ")", "read-latest", () => { void withBusy("read-latest", () => verifyRead(latest.id, undefined, "latest")); }));
    }
    panel.append(intro.element, noticeLine("first-result"));
    if (hits && hits.length) {
      const results = make("ul", undefined, "onboarding-hits");
      results.setAttribute("aria-label", "Search matches");
      for (const hit of hits) {
        const item = make("li");
        const button = createUiButton(doc, "", { variant: "quiet", className: "onboarding-hit" });
        button.dataset.focusKey = "hit-" + hit.source_id + "-" + hit.message_id;
        button.append(make("strong", hit.title || "Untitled conversation"), make("small", hit.role + " · " + hit.kind), make("span", hit.snippet));
        button.addEventListener("click", () => { if (alive) void withBusy("read-hit", () => verifyRead(hit.archive_id, hit.snapshot_id, "search", hit.message_id)); });
        item.append(button);
        results.append(item);
      }
      panel.append(results);
    }
    if (problem) {
      const state = createUiStatePanel(doc, { state: problem.state === "locked" ? "locked" : "error", title: problem.title, detail: problem.detail });
      panel.append(state);
    }
    if (verified) {
      const result = verified;
      const card = createUiCard(doc, { title: "First result verified", description: "Read from your archive just now. This is real saved content." });
      card.element.classList.add("onboarding-verified");
      card.element.dataset.verified = "true";
      const facts = make("dl", undefined, "onboarding-facts");
      facts.append(field("Conversation", result.title), field("Provider", titleCase(result.provider)), field("Captured", formatCapturedAt(result.capturedAt)), field("Revision", result.snapshotId, true), field("Found by", result.via === "search" ? "Search" : "Latest conversation"));
      const excerpt = make("blockquote", result.excerpt); excerpt.className = "onboarding-excerpt";
      card.body.append(createUiStatus(doc, "Verified read", "complete"), facts, excerpt, action("Open this conversation", "open-conversation", () => hooks.openConversation(result.archiveId, result.snapshotId === "latest revision" ? undefined : result.snapshotId)));
      panel.append(card.element);
    }
    panel.append(navigation(true, null));
    panel.append(finishRow(verified !== null));
  }
  function finishRow(isVerified: boolean): HTMLElement {
    const row = make("div", undefined, "onboarding-finish");
    if (!isVerified) row.append(make("p", "You can finish now and come back — setup completion does not mean a result was verified.", "onboarding-note"));
    row.append(action("Finish setup", "finish", () => {
      finishedAt = now().toISOString();
      persist();
      hooks.onFinish();
    }, isVerified ? "primary" : "secondary"));
    return row;
  }

  function renderStepper(): void {
    stepList.replaceChildren();
    for (const [index, definition] of ONBOARDING_STEPS.entries()) {
      const item = make("li");
      const button = make("button", undefined, "onboarding-step-button");
      button.type = "button";
      button.dataset.step = definition.id;
      button.dataset.focusKey = "step-" + definition.id;
      if (definition.id === step) button.setAttribute("aria-current", "step");
      const number = make("span", String(index + 1), "onboarding-step-number"); number.setAttribute("aria-hidden", "true");
      const text = make("span", undefined, "onboarding-step-text");
      text.append(make("span", definition.label, "onboarding-step-label"), make("small", definition.id === "providers" && providersSkipped ? "Optional · skipped" : definition.requirement));
      button.append(number, text);
      button.addEventListener("click", () => { if (alive) goTo(definition.id); });
      item.append(button);
      stepList.append(item);
    }
  }

  function render(): void {
    if (!alive) return;
    const active = doc.activeElement instanceof doc.defaultView!.HTMLElement ? doc.activeElement : null;
    const restoreKey = active && root.contains(active) ? active.dataset.focusKey ?? null : null;
    sub.textContent = finishedAt
      ? "You finished setup earlier. Review any step to re-check real status."
      : "A guided, local-first path from your vault to your first verified result.";
    counter.textContent = "Step " + (stepIndex(step) + 1) + " of " + ONBOARDING_STEPS.length;
    panel.replaceChildren();
    panel.dataset.step = step;
    panel.setAttribute("aria-busy", String(busy.size > 0));
    renderStepper();
    if (step === "welcome") renderWelcome();
    else if (step === "vault") renderVault();
    else if (step === "providers") renderProviders();
    else renderFirstResult();
    if (restoreKey) {
      for (const element of root.querySelectorAll<HTMLElement>("[data-focus-key]")) {
        if (element.dataset.focusKey === restoreKey) { element.focus({ preventScroll: true }); break; }
      }
    }
  }

  render();
  const ready = dataLoad();
  const state = (): UiModuleDataState<OnboardingSnapshot> => {
    // Only the sections the visible step depends on can make the module loading or failed.
    const relevant: Section<unknown>[] = step === "vault" ? [security] : step === "providers" ? [setup, mcp] : step === "first-result" ? [archive] : [];
    const failed = relevant.find((section) => section.status === "error");
    const lockedFailure = failed?.status === "error" && failed.locked;
    const vaultLocked = security.status === "ready" && security.value.locked;
    return {
      status: lockedFailure || vaultLocked ? "locked" : failed ? "error" : relevant.some((section) => section.status === "pending") ? "loading" : "ready",
      data: { step, providersSkipped, firstResultVerified: verified !== null },
      error: failed?.status === "error" ? failed.error : null,
      permission: "not-required",
      vault: security.status === "ready" ? (security.value.locked ? "locked" : "unlocked") : "not-applicable",
    };
  };

  return {
    element: root,
    ready,
    state,
    refresh: dataLoad,
    goTo: (next) => goTo(next, false),
    destroy: () => {
      alive = false;
      refreshVersion++;
      actionVersion++;
      security = PENDING; setup = PENDING; mcp = PENDING; archive = PENDING;
      verified = null; hits = null; problem = null; notice = null; query = "";
      root.replaceChildren();
      if (root.parentElement === view) root.remove();
    },
  };
}
