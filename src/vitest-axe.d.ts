// vitest-axe@0.1.0's own type-augmentation file targets the vitest 0.x `Vi`
// namespace; this project is on vitest 1.6, whose real target is
// `@vitest/expect`'s `Assertion` interface — augmenting that directly here
// since the package's own declaration doesn't reach it.
import type { AxeResults } from "axe-core";

declare module "@vitest/expect" {
  interface Assertion<T = unknown> {
    toHaveNoViolations(): T extends AxeResults ? void : never;
  }
}
