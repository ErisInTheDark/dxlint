# es-no-useless-constructor

Reports constructors that add nothing to a class's default construction behaviour. Constructors with parameter properties, decorators, or non-public visibility are preserved.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-useless-constructor`](https://typescript-eslint.io/rules/no-useless-constructor) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions` |
| Parity | 99 / 99 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-constructor
```

## Examples

Incorrect:

```ts
class Example {
  constructor() {}
}
class Derived extends Example {
  constructor(...args) {
    super(...args);
  }
}
```

Correct:

```ts
class Example {}
class Derived extends Example {}
```

## Differences from upstream

- The TypeScript upstream rule extends ESLint's core constructor check. dxlint includes the TypeScript exceptions in the same rule.
- dxlint reports without a constructor-removal suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-useless-constructor) and [ESLint](https://eslint.org/docs/latest/rules/no-useless-constructor) documentation (MIT).
