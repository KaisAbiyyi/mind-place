import { invoke } from "@tauri-apps/api/core";
import "katex/dist/katex.min.css";
import "./style.css";
import { mountReader, type Invoke } from "./reader.js";

const root = document.querySelector<HTMLElement>("#app");
if (!root) throw new Error("Mind Place reader root was not found.");

const tauriInvoke: Invoke = <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args);
mountReader({ root, invoke: tauriInvoke });
