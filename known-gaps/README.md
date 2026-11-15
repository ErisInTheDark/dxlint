# Known gaps

Fixture scenarios (same layout as `fixtures/`) whose expectation is right but that dxlint doesn't meet yet, ungated by the user's decision. `npm run check` runs them and reports the count without failing; a scenario that passes again moves back into `fixtures/` at its old path.

- `ts-oracle/*`: tsc's printed types for checker corners no rule needs (the type layer serves rules, not a full checker).
- Low-importance rule gaps: CommonJS/ESM interop of `import()` (`ts/typed/dynamic-import-*`), `any` inference past a conditional member (`engine/conditional-any-inference`), property contexts inside an argument still inferring `T` (`ts/typed-structure/captured/generic-enum-arguments-…-003`), explicit JSX type arguments (`ts-semantic/enums/enum-jsx-values-…-001`), contextual literal preservation in `es-no-unnecessary-type-arguments` (`ts-typed/contextual-type-arguments`).

Never move a scenario here to make a check pass without the user deciding it.
