import type { ClipboardWriter, Invoke } from "../../reader.js";
import { createConnectionsPage } from "../connections/page.js";
import type { Ui2PageMap } from "./page.js";

export interface Ui2PageServices {
  /** Epoch-fenced native call. A page never receives the unfenced invoke. */
  invoke: Invoke;
  clipboard?: ClipboardWriter;
}

/** Pages the shell can mount. Destinations not listed keep the "not ready" placeholder. */
export function createUi2Pages(services: Ui2PageServices): Ui2PageMap {
  return {
    connections: ({ doc, i18n }) =>
      createConnectionsPage({
        doc,
        i18n,
        invoke: services.invoke,
        ...(services.clipboard ? { clipboard: services.clipboard } : {}),
      }),
  };
}
