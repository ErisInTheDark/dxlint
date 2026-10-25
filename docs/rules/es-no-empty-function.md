# es-no-empty-function

Reports functions with empty bodies and no explanatory comment. An empty implementation can hide a missing step or an unintended no-op.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-empty-function`](https://typescript-eslint.io/rules/no-empty-function) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 26 / 26 corpus cases |

## Enable

```dx
import .ts
rule es-no-empty-function
```

## Examples

Incorrect:

```ts
function foo() {}
const bar = () => {};
```

Correct:

```ts
function foo() {
  // Intentionally empty.
}
const bar = () => 1;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `string[]` | `[]` | Function categories allowed to have empty bodies. |

Categories include `functions`, `arrowFunctions`, `generatorFunctions`, `methods`, `generatorMethods`, `getters`, `setters`, `constructors`, `asyncFunctions`, `asyncMethods`, `private-constructors`, `protected-constructors`, `decoratedFunctions` and `overrideMethods`. Constructors with parameter properties are allowed.

```dx
rule es-no-empty-function {
  set allow ["arrowFunctions", "private-constructors"]
}
```

## Differences from upstream

- The upstream TypeScript rule extends ESLint's core rule with TypeScript constructor and method exemptions.
- dxlint does not provide the upstream suggestion to insert a comment. Its diagnostic uses a shorter function description.
- `privateConstructors` and `protectedConstructors` are also accepted as aliases for the hyphenated categories.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-empty-function) documentation (MIT).
