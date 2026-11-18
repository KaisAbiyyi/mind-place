import {
  EN_CATALOG,
  ID_CATALOG,
  createI18nController,
  formatUserChatText,
  resolveCuratedMemoryCopy,
  type Ui2I18nController,
  type Ui2TestLocale,
} from "./index.js";

export interface MountI18nDemoOptions {
  i18n?: Ui2I18nController;
  sampleDate?: Date;
}

export interface I18nDemoHandle {
  destroy(): void;
  i18n: Ui2I18nController;
}

export function mountI18nDemo(
  root: HTMLElement,
  options: MountI18nDemoOptions = {}
): I18nDemoHandle {
  const doc = root.ownerDocument;
  const i18n = options.i18n ?? createI18nController();
  const sampleDate = options.sampleDate ?? new Date(Date.UTC(2026, 10, 14, 9, 30, 0));
  let showOriginalMemory = false;

  const container = doc.createElement("section");
  container.className = "ui2-i18n-demo";
  container.dataset.ui2I18nDemo = "true";

  const render = () => {
    container.replaceChildren();
    const locale = i18n.getLocale();
    doc.documentElement.lang = locale === "id" ? "id" : "en";

    const header = doc.createElement("header");
    const heading = doc.createElement("h1");
    heading.textContent = i18n.t("demo.title");
    const subtitle = doc.createElement("p");
    subtitle.textContent = i18n.t("demo.subtitle");
    header.append(heading, subtitle);

    const switcher = doc.createElement("div");
    switcher.setAttribute("role", "group");
    switcher.setAttribute("aria-label", i18n.t("top.languageLabel"));
    for (const candidate of ["en", "id", "pseudo"] as const satisfies readonly Ui2TestLocale[]) {
      const btn = doc.createElement("button");
      btn.type = "button";
      btn.dataset.demoLocale = candidate;
      btn.setAttribute("aria-pressed", String(locale === candidate));
      btn.textContent = i18n.t(`lang.short.${candidate}`);
      btn.addEventListener("click", () => {
        i18n.setLocale(candidate);
      });
      switcher.append(btn);
    }

    const summary = doc.createElement("div");
    summary.dataset.demoSummary = "true";
    const summaryLabel = doc.createElement("h2");
    summaryLabel.textContent = i18n.t("demo.sampleCountLabel");
    const countsLine = doc.createElement("p");
    countsLine.dataset.demoPlurals = "true";
    countsLine.textContent = [
      i18n.tPlural("count.chats", 1),
      i18n.tPlural("count.memories", 1240),
      i18n.tPlural("count.sources", 0),
    ].join(" · ");
    const dateLine = doc.createElement("p");
    dateLine.dataset.demoDate = "true";
    dateLine.textContent = `${i18n.t("demo.sampleDateLabel")}: ${i18n.formatDate(sampleDate, "medium")}`;
    const unknownLine = doc.createElement("p");
    unknownLine.dataset.demoUnknown = "true";
    unknownLine.textContent = i18n.formatNumber(Number.NaN);
    summary.append(summaryLabel, countsLine, dateLine, unknownLine);

    const memoryCard = doc.createElement("article");
    memoryCard.dataset.demoMemory = "true";
    const memoryResolution = resolveCuratedMemoryCopy(
      {
        statement: EN_CATALOG["gallery.sample.memoryStatement"],
        originalLocale: "en",
        translations: {
          id: ID_CATALOG["gallery.sample.memoryStatement"],
        },
      },
      locale,
      showOriginalMemory
    );
    const memoryText = doc.createElement("p");
    memoryText.dataset.demoMemoryText = "true";
    memoryText.textContent = memoryResolution.text;
    memoryCard.append(memoryText);
    if (memoryResolution.translatedFromLabel) {
      const meta = doc.createElement("div");
      const badge = doc.createElement("span");
      badge.dataset.demoTranslatedBadge = "true";
      badge.textContent = memoryResolution.translatedFromLabel;
      meta.append(badge);
      if (memoryResolution.toggleActionLabel) {
        const toggle = doc.createElement("button");
        toggle.type = "button";
        toggle.dataset.demoViewOriginal = "true";
        toggle.textContent = memoryResolution.toggleActionLabel;
        toggle.addEventListener("click", () => {
          showOriginalMemory = !showOriginalMemory;
          render();
        });
        meta.append(toggle);
      }
      memoryCard.append(meta);
    }

    const transcriptCard = doc.createElement("article");
    transcriptCard.dataset.demoTranscript = "true";
    const transcriptTitle = doc.createElement("h2");
    transcriptTitle.textContent = i18n.t("demo.userTranscriptLabel");
    const transcriptBody = doc.createElement("p");
    transcriptBody.dataset.demoTranscriptBody = "true";
    transcriptBody.textContent = formatUserChatText(EN_CATALOG["gallery.sample.userChatText"]);
    transcriptCard.append(transcriptTitle, transcriptBody);

    container.append(header, switcher, summary, memoryCard, transcriptCard);
  };

  const unsubscribe = i18n.subscribe(() => render());
  render();
  root.replaceChildren(container);

  return {
    i18n,
    destroy() {
      unsubscribe();
      container.replaceChildren();
      container.remove();
    },
  };
}
