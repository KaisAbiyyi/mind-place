import type { Invoke } from "../../reader.js";
import type { Ui2I18nController } from "../i18n/index.js";
import {
  createUi2Banner,
  createUi2Button,
  createUi2Disclosure,
  createUi2Field,
  createUi2SettingsRow,
  createUi2Switch,
} from "../ui-kit/primitives.js";
import type { ConnectionsSection } from "./section.js";

interface HostedStatus {
  running: boolean;
  tunnel_id: string | null;
  connection_verified: boolean;
}

export interface HostedDeps {
  doc: Document;
  i18n: Ui2I18nController;
  invoke: Invoke;
}

/**
 * Connections > Advanced: connect hosted ChatGPT (collapsed). Same consent and
 * credential semantics as the classic screen: the OpenAI key is cleared from
 * memory and the field the moment it is sent, explicit consent is required to
 * start, and a running helper is never described as a verified connection.
 */
/** Anything that is not a well-formed status is "unknown", never "stopped". */
function asStatus(value: unknown): HostedStatus | null {
  const candidate = value as Partial<HostedStatus> | null | undefined;
  return candidate && typeof candidate.running === "boolean" ? (candidate as HostedStatus) : null;
}

export function createHostedSection(deps: HostedDeps): ConnectionsSection {
  const { doc, i18n, invoke } = deps;
  const root = doc.createElement("div");

  let alive = true;
  let open = false;
  let status: HostedStatus | null = null;
  let busy = false;
  let message: string | null = null;
  let tunnelId = "";
  let apiKey = "";
  let consent = false;

  function render(): void {
    if (!alive) return;
    const body = doc.createElement("div");
    body.className = "ui2-stack";

    const intro = doc.createElement("p");
    intro.className = "ui2-field-hint";
    intro.textContent = i18n.t("conn.advV");
    body.append(intro);

    if (message) {
      body.append(createUi2Banner(doc, { tone: "danger", title: message, message: i18n.t("state.errorHint") }));
    }

    const idField = createUi2Field(doc, {
      label: i18n.t("conn.tunnelId"),
      value: tunnelId,
      onInput: (value) => {
        tunnelId = value;
        syncStart();
      },
    });
    idField.input.autocomplete = "off";
    idField.input.spellcheck = false;
    const keyField = createUi2Field(doc, {
      label: i18n.t("conn.tunnelKey"),
      hint: i18n.t("conn.tunnelKeyHint"),
      type: "password",
      value: apiKey,
      onInput: (value) => {
        apiKey = value;
        syncStart();
      },
    });
    keyField.input.autocomplete = "off";
    keyField.input.spellcheck = false;
    body.append(idField.element, keyField.element);

    body.append(
      createUi2SettingsRow(doc, {
        title: i18n.t("conn.consent"),
        control: createUi2Switch(doc, {
          ariaLabel: i18n.t("conn.consent"),
          checked: consent,
          onChange: (checked) => {
            consent = checked;
            syncStart();
          },
        }),
      }),
    );

    const actions = doc.createElement("div");
    actions.className = "ui2-row-flex";
    const start = createUi2Button(doc, {
      label: i18n.t("conn.tunnelStart"),
      variant: "primary",
      onClick: () => void startTunnel(),
    });
    const stop = createUi2Button(doc, {
      label: i18n.t("conn.tunnelStop"),
      variant: "secondary",
      disabled: busy || status?.running !== true,
      onClick: () => void stopTunnel(),
    });
    actions.append(start, stop);
    body.append(actions);

    function syncStart(): void {
      start.disabled = !canStart();
    }
    syncStart();

    const state = doc.createElement("p");
    state.className = "ui2-field-hint";
    state.setAttribute("role", "status");
    state.textContent =
      status === null ? i18n.t("state.unknown") : i18n.t(status.running ? "conn.tunnelRunning" : "conn.tunnelStopped");
    body.append(state);

    const how = doc.createElement("p");
    how.className = "ui2-field-hint";
    how.textContent = i18n.t("conn.tunnelHow");
    body.append(how);

    const disclosure = createUi2Disclosure(doc, {
      summaryLabel: i18n.t("conn.advH"),
      body,
      defaultOpen: open,
    });
    disclosure.addEventListener("toggle", () => {
      open = disclosure.open;
    });
    root.replaceChildren(disclosure);
  }

  function canStart(): boolean {
    return !busy && consent && tunnelId.trim().length > 0 && apiKey.length > 0;
  }

  async function refresh(): Promise<void> {
    try {
      status = asStatus(await invoke<HostedStatus>("get_hosted_tunnel_status"));
    } catch {
      status = null;
    }
    render();
  }

  async function startTunnel(): Promise<void> {
    if (!canStart()) return;
    busy = true;
    message = null;
    // Send, then clear: the key never stays in memory or in the field.
    const sentKey = apiKey;
    const sentId = tunnelId.trim();
    apiKey = "";
    render();
    try {
      status = asStatus(
        await invoke<HostedStatus>("start_hosted_tunnel", {
          tunnelId: sentId,
          apiKey: sentKey,
          acknowledgedRemoteAccess: consent,
        }),
      );
    } catch (error) {
      message = /MP_TUNNEL_(BINARY|INTEGRITY):/.test(String(error))
        ? i18n.t("conn.tunnelBinary")
        : i18n.t("conn.tunnelError");
    }
    busy = false;
    if (!alive) return;
    await refresh();
  }

  async function stopTunnel(): Promise<void> {
    busy = true;
    message = null;
    render();
    try {
      status = asStatus(await invoke<HostedStatus>("stop_hosted_tunnel"));
    } catch {
      message = i18n.t("conn.tunnelStopError");
    }
    busy = false;
    if (!alive) return;
    await refresh();
  }

  render();
  void refresh();

  return {
    element: root,
    rebuild: render,
    destroy() {
      alive = false;
      // Nothing secret survives the page: key, id and consent are dropped.
      apiKey = "";
      tunnelId = "";
      consent = false;
      root.replaceChildren();
    },
  };
}
