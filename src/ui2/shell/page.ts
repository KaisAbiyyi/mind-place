import type { Ui2I18nController } from "../i18n/index.js";
import type { Ui2Destination } from "../ui-kit/index.js";

/**
 * A destination page mounted inside the shell (MP-UI2-09 and later tasks).
 * The shell owns navigation, the top bar and the page heading; a page owns
 * everything below the heading. A page is created when the owner opens its
 * destination and destroyed when they leave it or the shell is destroyed
 * (Lock), so it must clear anything secret in `destroy()`.
 */
export interface Ui2PageHandle {
  readonly element: HTMLElement;
  destroy(): void;
}

export interface Ui2PageContext {
  readonly doc: Document;
  readonly i18n: Ui2I18nController;
}

export type Ui2PageFactory = (context: Ui2PageContext) => Ui2PageHandle;
export type Ui2PageMap = Partial<Record<Ui2Destination, Ui2PageFactory>>;
