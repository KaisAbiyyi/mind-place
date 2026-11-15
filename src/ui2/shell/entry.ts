// This module is imported after native unlock only for the owner-enabled v2 preview.
// Vite injects these stylesheet chunks in the browser; Node unit tests can import
// the shell without attempting to load CSS as a Node module.
if (import.meta.env) {
  void import("../ui-kit/tokens.css");
  void import("../ui-kit/primitives.css");
  void import("./shell.css");
}
export { mountUi2Shell } from "./shell.js";
