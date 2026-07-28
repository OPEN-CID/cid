// 051-Editor-Excellence-Roadmap.md Wave 5.5: scaffolding, not a full
// translation rollout — a real `t()` lookup plus an English catalogue,
// applied to the shared UI chrome (dialogs, command palette, modals) that
// most other strings will eventually route through. English values are kept
// byte-identical to what was previously hardcoded, so this introduces no
// user-visible or test-visible change; it only adds the seam a second locale
// would plug into. Most component-specific copy (panel body text, form
// labels throughout Settings/Autonomy/Health/etc.) is intentionally not
// migrated yet — that's real, scoped follow-on work, not silently declared
// done here.

const en = {
  common: {
    save: "Save",
    cancel: "Cancel",
    confirm: "Confirm",
    close: "Close",
    delete: "Delete",
    discard: "Discard",
  },
  dialog: {
    unsavedChangesTitle: "Unsaved changes",
  },
  editor: {
    unsavedChangesBody: "has unsaved edits. Closing it will lose them unless you save first.",
    saveAndClose: "Save & close",
    discardAndClose: "Discard & close",
  },
  mission: {
    newMission: "New Mission",
  },
  commandPalette: {
    placeholder: "Type a command…",
    ariaLabel: "Command palette",
    searchAriaLabel: "Search commands",
    noMatches: "No matching commands",
  },
  shortcuts: {
    title: "Keyboard shortcuts",
  },
} as const;

type Catalogue = typeof en;
const catalogues: Record<"en", Catalogue> = { en };

// Single-locale today (English only) — the function shape (a locale lookup
// keyed by a stable id) is what makes adding a second locale later a data
// change, not a rewrite of every call site.
let currentLocale: keyof typeof catalogues = "en";

export function setLocale(locale: keyof typeof catalogues) {
  currentLocale = locale;
}

export function t(): Catalogue {
  return catalogues[currentLocale];
}
