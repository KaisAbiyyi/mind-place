import type { ClipboardWriter, Invoke } from "../../reader.js";
import type { Ui2PageContext, Ui2PageHandle } from "../shell/page.js";
import { createAiAccessSection } from "./ai-access.js";
import { createCaptureSection } from "./capture.js";
import { createHostedSection } from "./hosted.js";

export interface ConnectionsPageDeps extends Ui2PageContext {
  invoke: Invoke;
  clipboard?: ClipboardWriter;
}

/** MP-UI2-09: the Connections destination (capture options and AI app access). */
export function createConnectionsPage(deps: ConnectionsPageDeps): Ui2PageHandle {
  const { doc, i18n, invoke, clipboard } = deps;
  const root = doc.createElement("div");
  root.className = "ui2-stack";

  const lead = doc.createElement("p");
  lead.className = "ui2-lead";

  const capture = createCaptureSection({ doc, i18n, invoke });
  const hosted = createHostedSection({ doc, i18n, invoke });
  const aiAccess = createAiAccessSection({ doc, i18n, invoke, hosted, ...(clipboard ? { clipboard } : {}) });

  const paintLead = () => {
    lead.textContent = i18n.t("conn.lead");
  };
  paintLead();
  root.append(lead, capture.element, aiAccess.element);

  // A language change repaints text in place; sections keep their state, so a
  // key that was just issued is not lost by switching language.
  const unsubscribe = i18n.subscribe(() => {
    paintLead();
    capture.rebuild();
    aiAccess.rebuild();
  });

  return {
    element: root,
    destroy() {
      unsubscribe();
      capture.destroy();
      aiAccess.destroy();
      root.replaceChildren();
      root.remove();
    },
  };
}
