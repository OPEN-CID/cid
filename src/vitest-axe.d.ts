// vitest-axe@0.1.0's own type-augmentation file targets the vitest 0.x `Vi`
// namespace. Through vitest 4 the real target was `@vitest/expect`'s
// `Assertion` interface (augmented here directly); vitest 5 builds its
// `Assertion` by mapping over the global `Chai.Assertion` instead, so both
// older targets miss and `toHaveNoViolations()` fails type-check — same
// drift class as `vitest-jest-dom.d.ts`, same fix.
declare global {
  namespace Chai {
    interface Assertion {
      toHaveNoViolations(): void;
    }
  }
}

// `void` rather than the old `T extends AxeResults ? void : never`
// conditional: vitest 5 maps matchers through `Chai.Assertion` structurally,
// and a conditional return on the base breaks the mapped copies. The runtime
// still only passes axe results — the type just no longer proves it.
export {};
