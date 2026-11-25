/** One section of the Connections page. Pages rebuild a section when the language changes. */
export interface ConnectionsSection {
  readonly element: HTMLElement;
  rebuild(): void;
  destroy(): void;
}
