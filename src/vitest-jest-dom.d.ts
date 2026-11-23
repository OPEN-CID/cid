// @testing-library/jest-dom augments the `vitest` module's `Assertion`
// interface, which is what `expect()` returned through vitest 4. Vitest 5
// builds its `Assertion` by mapping over the global `Chai.Assertion`
// instead (`VitestAssertion<Chai.Assertion, …>`), so the package
// augmentation no longer reaches and every `toBeInTheDocument()` fails
// type-check — same drift class as `vitest-axe.d.ts`, same fix: augment
// the real target directly. The interface is reached through the package's
// exported `./matchers` subpath rather than re-declared here, so matcher
// signatures stay single-sourced.
import type { TestingLibraryMatchers } from "@testing-library/jest-dom/matchers";
declare global {
  namespace Chai {
    // Both disables are the augmentation mechanism, not sloppiness: the
    // interface must stay memberless to merge with Chai.Assertion, and the
    // `any`s mirror TestingLibraryMatchers' own variance (<any, any>).
    // eslint-disable-next-line @typescript-eslint/no-empty-object-type, @typescript-eslint/no-explicit-any
    interface Assertion extends TestingLibraryMatchers<any, any> {}
  }
}

export {};
