import {
  EN_CATALOG,
  ID_CATALOG,
  createI18nController,
  formatUserChatText,
  resolveCuratedMemoryCopy,
  type Ui2I18nController,
  type Ui2TestLocale,
} from "../i18n/index.js";
import { ensureUi2IconSprite } from "./icons.js";
import {
  createUi2Aside,
  createUi2Banner,
  createUi2Breadcrumb,
  createUi2Button,
  createUi2Card,
  createUi2ChatMessage,
  createUi2ChipGroup,
  createUi2Dialog,
  createUi2Disclosure,
  createUi2EmptyState,
  createUi2Field,
  createUi2InlineConfirm,
  createUi2ListCard,
  createUi2ListRow,
  createUi2LoadingSkeleton,
  createUi2MemoryCard,
  createUi2Segmented,
  createUi2SettingsCard,
  createUi2StatCard,
  createUi2Switch,
  createUi2Tag,
  createUi2TechnicalDetails,
  createUi2Toast,
} from "./primitives.js";
import type { Ui2DensityMode, Ui2ThemeMode } from "./tokens.js";

export interface MountUi2GalleryOptions {
  i18n?: Ui2I18nController;
  initialTheme?: Ui2ThemeMode;
  initialDensity?: Ui2DensityMode;
}

export interface Ui2GalleryHandle {
  i18n: Ui2I18nController;
  setTheme(mode: Ui2ThemeMode): void;
  setDensity(mode: Ui2DensityMode): void;
  destroy(): void;
}

export function mountUi2Gallery(
  root: HTMLElement,
  options: MountUi2GalleryOptions = {}
): Ui2GalleryHandle {
  const doc = root.ownerDocument;
  ensureUi2IconSprite(doc);
  const i18n = options.i18n ?? createI18nController();

  let theme: Ui2ThemeMode = options.initialTheme ?? "light";
  let density: Ui2DensityMode = options.initialDensity ?? "comfortable";
  let selectedChip = "all";
  let switchState = false;
  let showOriginalMemory = false;
  const sampleUpdatedAt = new Date(Date.UTC(2026, 10, 14, 14, 0, 0));

  const shell = doc.createElement("main");
  shell.className = "ui2-root";

  const applyThemeAndDensity = () => {
    const html = doc.documentElement;
    if (theme === "system") {
      html.removeAttribute("data-theme");
    } else {
      html.setAttribute("data-theme", theme);
    }
    html.setAttribute("data-density", density);
  };

  let activeDialog = createUi2Dialog(doc, {
    title: i18n.t("gallery.sample.dialogTitle"),
    closeLabel: i18n.t("action.close"),
  });
  let activeAside = createUi2Aside(doc, {
    title: i18n.t("gallery.sample.asideTitle"),
    closeLabel: i18n.t("action.close"),
  });

  const render = () => {
    applyThemeAndDensity();
    activeDialog.destroy();
    shell.replaceChildren();
    doc.documentElement.lang = i18n.getLocale() === "id" ? "id" : "en";

    const wrap = doc.createElement("div");
    wrap.className = "ui2-wrap";

    // Header & breadcrumb
    const header = doc.createElement("header");
    header.className = "ui2-header";
    const breadcrumb = createUi2Breadcrumb(doc, {
      ariaLabel: i18n.t("breadcrumb.ariaLabel"),
      items: [
        { id: "home", label: i18n.t("nav.home"), onClick: () => {} },
        { id: "settings", label: i18n.t("nav.settings"), onClick: () => {} },
        { id: "gallery", label: i18n.t("gallery.title") },
      ],
    });
    const h1 = doc.createElement("h1");
    h1.className = "ui2-h1";
    h1.textContent = i18n.t("gallery.title");
    const lead = doc.createElement("p");
    lead.className = "ui2-lead";
    lead.textContent = i18n.t("gallery.subtitle");
    header.append(breadcrumb, h1, lead);

    // Controls card (Theme, Language, Density)
    const controlsCard = createUi2SettingsCard(doc, {
      title: i18n.t("gallery.controls.title"),
      rows: [
        {
          title: i18n.t("settings.language"),
          control: createUi2Segmented<Ui2TestLocale>(doc, {
            ariaLabel: i18n.t("top.languageLabel"),
            options: [
              { value: "en", label: i18n.t("lang.short.en") },
              { value: "id", label: i18n.t("lang.short.id") },
              { value: "pseudo", label: i18n.t("lang.short.pseudo") },
            ],
            value: i18n.getLocale(),
            onChange: (next) => i18n.setLocale(next),
          }),
        },
        {
          title: i18n.t("gallery.controls.theme"),
          control: createUi2Segmented<Ui2ThemeMode>(doc, {
            ariaLabel: i18n.t("gallery.controls.theme"),
            options: [
              { value: "light", label: i18n.t("settings.theme.light") },
              { value: "dark", label: i18n.t("settings.theme.dark") },
              { value: "system", label: i18n.t("settings.theme.system") },
            ],
            value: theme,
            onChange: (next) => {
              theme = next;
              applyThemeAndDensity();
            },
          }),
        },
        {
          title: i18n.t("gallery.controls.density"),
          control: createUi2Segmented<Ui2DensityMode>(doc, {
            ariaLabel: i18n.t("gallery.controls.density"),
            options: [
              { value: "comfortable", label: i18n.t("gallery.controls.densityComfort") },
              { value: "compact", label: i18n.t("gallery.controls.densityCompact") },
            ],
            value: density,
            onChange: (next) => {
              density = next;
              applyThemeAndDensity();
            },
          }),
        },
      ],
    });

    // Section 1: Buttons, chips, tags, switch, field
    const section1 = createUi2Card(doc, {
      title: i18n.t("gallery.section.buttonsChipsTags"),
    });
    const btnRow = doc.createElement("div");
    btnRow.className = "ui2-row-flex";
    const toast = createUi2Toast(doc);
    btnRow.append(
      createUi2Button(doc, {
        label: i18n.t("action.approve"),
        variant: "primary",
        icon: "check",
        onClick: () => toast.show(i18n.t("gallery.sample.toastCopied")),
      }),
      createUi2Button(doc, {
        label: i18n.t("action.browse"),
        variant: "secondary",
        icon: "folder",
      }),
      createUi2Button(doc, {
        label: i18n.t("action.copy"),
        variant: "ghost",
        icon: "copy",
      }),
      createUi2Button(doc, {
        label: i18n.t("action.revoke"),
        variant: "danger",
        icon: "x",
      })
    );

    const chipGroup = createUi2ChipGroup(doc, {
      ariaLabel: i18n.t("archive.title"),
      activeId: selectedChip,
      items: [
        { id: "all", label: i18n.t("gallery.sample.chipAll") },
        { id: "chatgpt", label: i18n.t("gallery.sample.chipChatgpt") },
        { id: "claude", label: i18n.t("gallery.sample.chipClaude") },
        { id: "gemini", label: i18n.t("gallery.sample.chipGemini") },
      ],
      onChange: (id) => {
        selectedChip = id;
      },
    });

    const tagRow = doc.createElement("div");
    tagRow.className = "ui2-row-flex";
    tagRow.append(
      createUi2Tag(doc, { label: i18n.t("gallery.sample.tagStable"), tone: "ok" }),
      createUi2Tag(doc, { label: i18n.t("gallery.sample.tagAttention"), tone: "warn" }),
      createUi2Tag(doc, { label: i18n.t("gallery.sample.tagMuted"), tone: "muted" }),
      createUi2Switch(doc, {
        ariaLabel: i18n.t("connections.autoSave"),
        checked: switchState,
        onChange: (val) => {
          switchState = val;
        },
      })
    );

    const searchField = createUi2Field(doc, {
      label: i18n.t("memory.title"),
      placeholder: i18n.t("memory.searchPlaceholder"),
      hint: i18n.t("settings.languageHint"),
    });
    section1.body.append(btnRow, chipGroup, tagRow, searchField.element);

    // Section 2: Stat cards, List card, Memory card, Chat message
    const section2 = createUi2Card(doc, {
      title: i18n.t("gallery.section.rowsAndCards"),
    });
    const statGrid = doc.createElement("div");
    statGrid.className = "ui2-stat-grid";
    statGrid.append(
      createUi2StatCard(doc, {
        label: i18n.t("home.stat.chats"),
        value: i18n.tPlural("count.chats", 42),
      }),
      createUi2StatCard(doc, {
        label: i18n.t("home.stat.memories"),
        value: i18n.tPlural("count.memories", 18),
      }),
      createUi2StatCard(doc, {
        label: i18n.t("home.stat.sources"),
        value: i18n.tPlural("count.sources", 1),
      })
    );

    const listCard = createUi2ListCard(doc, [
      createUi2ListRow(doc, {
        title: i18n.t("gallery.sample.folderName"),
        meta: i18n.formatDate(sampleUpdatedAt, "medium"),
        icon: "folder",
        tag: { label: i18n.t("sources.status.approved"), tone: "ok" },
        onClick: () => {},
      }),
      createUi2ListRow(doc, {
        title: i18n.t("gallery.sample.clientName"),
        meta: i18n.t("connections.mcpStatusRunning"),
        icon: "link",
        tag: { label: i18n.t("gallery.sample.tagMuted"), tone: "muted" },
        onClick: () => {},
      }),
    ]);

    const memCopy = resolveCuratedMemoryCopy(
      {
        statement: EN_CATALOG["gallery.sample.memoryStatement"],
        originalLocale: "en",
        translations: {
          id: ID_CATALOG["gallery.sample.memoryStatement"],
        },
      },
      i18n.getLocale(),
      showOriginalMemory
    );
    const memoryCard = createUi2MemoryCard(doc, {
      statement: memCopy.text,
      meta: i18n.formatDate(sampleUpdatedAt, "short"),
      statusTag: { label: i18n.t("memory.disposition.current_candidate"), tone: "ok" },
      translatedFromLabel: memCopy.translatedFromLabel,
      toggleOriginalLabel: memCopy.toggleActionLabel,
      onToggleOriginal: () => {
        showOriginalMemory = !showOriginalMemory;
        render();
      },
      actionLabel: i18n.t("memory.behindThis"),
      onInspect: () => activeAside.open(),
    });

    const chatMsg = createUi2ChatMessage(doc, {
      role: "user",
      roleLabel: i18n.t("archive.role.user"),
      timestampLabel: i18n.formatDate(sampleUpdatedAt, "datetime"),
      text: formatUserChatText(EN_CATALOG["gallery.sample.userChatText"]),
    });

    section2.body.append(statGrid, listCard, memoryCard, chatMsg);

    // Section 3: States & feedback (loading, empty, error, partial, stale, inline confirm)
    const section3 = createUi2Card(doc, {
      title: i18n.t("gallery.section.statesAndFeedback"),
    });
    const inlineConfirm = createUi2InlineConfirm(doc, {
      triggerLabel: i18n.t("gallery.sample.inlineTrigger"),
      question: i18n.t("gallery.sample.inlineQuestion"),
      confirmLabel: i18n.t("action.confirm"),
      cancelLabel: i18n.t("action.cancel"),
      onConfirm: () => toast.show(i18n.t("gallery.sample.inlineRemoved")),
    });

    section3.body.append(
      createUi2LoadingSkeleton(doc, i18n.t("state.loading"), i18n),
      createUi2EmptyState(doc, {
        icon: "memory",
        title: i18n.t("state.empty"),
        body: i18n.t("state.emptyHint"),
        actionLabel: i18n.t("home.quick.import"),
        onAction: () => {},
      }),
      createUi2Banner(doc, {
        tone: "danger",
        title: i18n.t("state.error"),
        message: i18n.t("state.errorHint"),
        actionLabel: i18n.t("action.retry"),
        onAction: () => {},
      }),
      createUi2Banner(doc, {
        tone: "warn",
        title: i18n.t("state.partial"),
        message: i18n.t("state.partialHint"),
      }),
      createUi2Banner(doc, {
        tone: "neutral",
        title: i18n.t("state.stale"),
        message: i18n.t("state.staleHint"),
        actionLabel: i18n.t("action.refresh"),
        onAction: () => {},
      }),
      inlineConfirm.element
    );

    // Section 4: Dialog, Aside, Toast, Disclosure, Hidden Technical Details
    const section4 = createUi2Card(doc, {
      title: i18n.t("gallery.section.overlaysAndDetails"),
    });
    activeDialog = createUi2Dialog(doc, {
      title: i18n.t("gallery.sample.dialogTitle"),
      closeLabel: i18n.t("action.close"),
    });
    const dlgBody = doc.createElement("p");
    dlgBody.className = "ui2-lead";
    dlgBody.textContent = i18n.t("gallery.sample.dialogBody");
    activeDialog.body.append(dlgBody);
    activeDialog.footer.append(
      createUi2Button(doc, {
        label: i18n.t("action.cancel"),
        variant: "secondary",
        onClick: () => activeDialog.close(),
      }),
      createUi2Button(doc, {
        label: i18n.t("action.approve"),
        variant: "primary",
        onClick: () => {
          activeDialog.close();
          toast.show(i18n.t("gallery.sample.toastCopied"));
        },
      })
    );

    activeAside = createUi2Aside(doc, {
      title: i18n.t("gallery.sample.asideTitle"),
      closeLabel: i18n.t("action.close"),
    });
    const asideCopy = doc.createElement("p");
    asideCopy.className = "ui2-lead";
    asideCopy.textContent = i18n.t("gallery.sample.asideBody");
    activeAside.body.append(asideCopy);

    const openDialogBtn = createUi2Button(doc, {
      label: i18n.t("gallery.sample.openDialog"),
      variant: "secondary",
      onClick: () => activeDialog.open(openDialogBtn),
    });
    const openAsideBtn = createUi2Button(doc, {
      label: i18n.t("gallery.sample.openAside"),
      variant: "secondary",
      onClick: () => activeAside.open(openAsideBtn),
    });
    const triggerToastBtn = createUi2Button(doc, {
      label: i18n.t("gallery.sample.triggerToast"),
      variant: "ghost",
      onClick: () => toast.show(i18n.t("gallery.sample.toastCopied")),
    });

    const overlayActions = doc.createElement("div");
    overlayActions.className = "ui2-row-flex";
    overlayActions.append(openDialogBtn, openAsideBtn, triggerToastBtn);

    const disclosure = createUi2Disclosure(doc, {
      summaryLabel: i18n.t("gallery.sample.disclosureSummary"),
      body: i18n.t("gallery.sample.disclosureBody"),
    });

    const techDetails = createUi2TechnicalDetails(doc, {
      summaryLabel: i18n.t("action.showDetails"),
      entries: [
        {
          label: i18n.t("gallery.sample.techLabel"),
          value: "sha256:2b4bbce50a079516e7db2326345cfd0bd227ff7b6a5dd347616fc9c790bc3a8c",
        },
        {
          label: i18n.t("archive.versions"),
          value: "rev_0042 · MP_VAULT_READY",
        },
      ],
    });

    section4.body.append(
      overlayActions,
      toast.element,
      disclosure,
      techDetails,
      activeAside.element,
      activeDialog.backdrop
    );

    wrap.append(
      header,
      controlsCard,
      section1.element,
      section2.element,
      section3.element,
      section4.element
    );
    shell.append(wrap);
  };

  const unsubscribe = i18n.subscribe(() => render());
  render();
  root.replaceChildren(shell);

  return {
    i18n,
    setTheme(next: Ui2ThemeMode) {
      theme = next;
      render();
    },
    setDensity(next: Ui2DensityMode) {
      density = next;
      render();
    },
    destroy() {
      unsubscribe();
      activeDialog.destroy();
      shell.replaceChildren();
      shell.remove();
    },
  };
}
