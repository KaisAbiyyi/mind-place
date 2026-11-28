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

/**
 * A page that opens one record (for example a chat) tells the shell its title
 * so the breadcrumb reads `Home > Archive > title`. The page then owns the
 * heading. `onBack` runs when the owner picks the destination crumb.
 */
export interface Ui2PageDetail {
  readonly title: string;
  onBack(): void;
}

export interface Ui2PageContext {
  readonly doc: Document;
  readonly i18n: Ui2I18nController;
  /** Optional: only pages with a detail view use it. Passing null returns to the destination root. */
  setDetail?(detail: Ui2PageDetail | null): void;
  /** Optional: go to another destination by its static path. */
  navigate?(path: string): void;
}

export type Ui2PageFactory = (context: Ui2PageContext) => Ui2PageHandle;
export type Ui2PageMap = Partial<Record<Ui2Destination, Ui2PageFactory>>;
