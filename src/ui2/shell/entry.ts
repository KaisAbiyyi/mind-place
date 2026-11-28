// CSS is an opt-in browser chunk. Never load these global UI2 tokens into v1.
export async function ensureUi2Styles(): Promise<void> {
  if (import.meta.env) {
    await Promise.all([
      import("../ui-kit/tokens.css"),
      import("../ui-kit/primitives.css"),
      import("./shell.css"),
      import("../connections/connections.css"),
      import("../chats/chats.css"),
    ]);
  }
}
export { fenceUi2EpochInvoke, mountUi2Shell } from "./shell.js";
export { createUi2Pages } from "./pages.js";
