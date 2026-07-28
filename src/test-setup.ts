import "@testing-library/jest-dom";
import { afterEach, expect } from "vitest";
import { cleanup } from "@testing-library/react";
// vitest-axe@0.1.0's own dist/extend-expect.js ships as an empty file, and
// both its extend-expect.d.ts and the package-root matchers.d.ts re-export
// everything as `export type *` — so the documented "vitest-axe/matchers"
// import type-checks as type-only even though the runtime export is real.
// Importing the untouched dist file directly skips that broken root
// redirect; the matching type augmentation lives in src/vitest-axe.d.ts,
// targeting @vitest/expect directly since the package's own declaration
// augments an older `Vi` namespace this vitest version doesn't use.
import { toHaveNoViolations } from "vitest-axe/dist/matchers";

expect.extend({ toHaveNoViolations });

// Without this, `render()` calls across multiple `it()` blocks in the same
// file never unmount — every prior test's component instance stays mounted
// and keeps reacting to state/effects, silently stealing mock calls meant
// for a later test (discovered via PlanCard.test.tsx: a later test's mock
// queue got consumed by earlier tests' still-mounted instances re-running
// effects). Single-test-per-file suites never surfaced this.
afterEach(() => {
  cleanup();
});
