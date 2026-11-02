# MP-UI-01 — Shared UI contract

**Task:** MP-UI-01  
**Repository:** \`KaisAbiyyi/mind-place\`  
**Base:** \`main @ efe91155d37d8b1ca0608fe3bd862be311fed004\`  
**Branch:** \`codex/mp-ui-01-shell-20261103\`  
**Resource lock:** \`ui-design-tokens/shared-primitives/app-shell/route-registry\`  
**Execution:** strict cloud-only

## Primary route registry

The shared shell exposes exactly eight primary destinations:

| Route ID | Label | Legacy workspace mapping |
|---|---|---|
| \`overview\` | Overview | Library search is a secondary action |
| \`conversations\` | Conversations | \`conversations\` |
| \`curated-memory\` | Curated Memory | \`memories\`, \`projects\`, \`people\`, \`topics\` |
| \`hive-mind-graph\` | Hive Mind Graph | \`graph\` |
| \`files-secure-vault\` | Files & Secure Vault | \`documents\` |
| \`ai-providers-mcp\` | AI Providers & MCP | \`connections\` |
| \`setup-onboarding\` | Setup & Onboarding | Existing archive/capture setup |
| \`settings\` | Settings | \`health\` is a secondary diagnostics action |

Legacy workspace IDs remain internal aliases. Projects, People and Topics are secondary lenses inside Curated Memory; diagnostics is a secondary Settings action; source search is a secondary Overview action.

## Route and module contracts

\`src/ui-contracts.ts\` exports typed route IDs, legacy workspace aliases, the route registry, a module registry, mount context/handle contracts, and a data state with explicit loading/ready/empty/error, permission and vault states.

Example:

\`\`\`ts
const memoryModule: UiModuleRegistration = {
  id: "curated-memory",
  route: "curated-memory",
  title: "Curated Memory",
  availability: "available",
  mount: ({ root, invoke, signal }) => mountCuratedMemory({ root, invoke, signal }),
};
registry.register(memoryModule);
\`\`\`

The initial `UI_MODULES` descriptors mark the already-reachable legacy surfaces as available; this does not claim that every legacy surface has been migrated to an independent `mount()` adapter yet. Modules can register searchable command-palette actions through `UiModuleContext.registerCommand`; the shell emits the registered command to the owning reader/module dispatcher. The default palette includes source search, Projects, People, Topics and diagnostics. Feature lanes can register their adapter without changing the route ID. The module's state must report permission and vault state from the native/backend contract. A disabled control, theme preference, technical-mode flag or route registration is never evidence of authorization or an unlocked vault.

## Shared primitives

\`src/ui-primitives.ts\` provides safe DOM factories for buttons, cards, status labels, tables, dialogs, drawers and explicit loading/error/permission states. Factories use \`textContent\` for caller-provided strings; untrusted content must not be inserted with \`innerHTML\`.

## Design tokens

The Notion ground-truth page explicitly records these source-verified anchors:

| Token | Light | Dark |
|---|---|---|
| App background | \`#F4F4F5\` | \`#09090B\` |
| Surface | \`#FFFFFF\` | \`#121215\` |
| Primary text | \`#09090B\` | \`#FAFAFA\` |

It also records Inter/system sans, JetBrains Mono/system monospace, radii 5/7/10/14 px, and responsive breakpoints 1120/780 px. Supporting neutral ramp and spacing aliases in \`src/ui-tokens.css\` are derived semantic tokens, not claimed as a byte-for-byte extraction of the uploaded HTML.

**Source-fidelity limitation:** the Notion page records the uploaded HTML filename, size and SHA-256, but the current Notion connector exposes only the page's text summary; attachment bytes/download URL are not available to this execution. The prior warm-gray prototype in the Library is not the current monochrome source and was deliberately not used. Exact full-token extraction and pixel comparison remain blocked until the owner HTML is available in the repository or current conversation.

## Shell behavior

- Eight primary routes; Projects/People/Topics, source search and diagnostics remain reachable as contextual secondary actions.
- Sidebar expanded/collapsed, responsive layout at 1120/780 px, breadcrumb, route command palette, theme/density/technical display preferences.
- Theme, density and technical mode are presentation preferences only; they are not backend authority.
- Conversation archive remains visible in the sidebar only on its route; the transcript reader remains the existing functional surface.
- Existing native commands, lock flow, permission gates and archive provenance remain owned by their current modules.

## Verification

Unit tests cover route ownership, aliases, module registry duplicate rejection, accessible primitives, shell preferences, command palette and route navigation. GitHub Actions is the authoritative cloud test environment; local checkout, terminal, owner device and native Windows qualification are out of scope.
