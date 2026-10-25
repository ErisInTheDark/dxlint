# es-no-unnecessary-type-assertion

Reports assertions that do not change an expression's type. This includes redundant non-null assertions.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-type-assertion`](https://typescript-eslint.io/rules/no-unnecessary-type-assertion) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 247 / 263 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-type-assertion
```

## Examples

Incorrect:

```ts
function foo(x: number): number {
  return x!;
}
```

Correct:

```ts
function foo(x: number): number {
  return x;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `typesToIgnore` | string[] | `[]` | Written assertion type names to leave alone. |
| `checkLiteralConstAssertions` | boolean | `false` | Also check const assertions on literal values. |

```dx
rule es-no-unnecessary-type-assertion {
  set checkLiteralConstAssertions true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unnecessary-type-assertion` | `as` and angle-bracket assertions. |
| `es-no-unnecessary-type-assertion:non-null` | Non-null assertions. |

`rule es-no-unnecessary-type-assertion` turns on every variant; `rule es-no-unnecessary-type-assertion:non-null off` turns one off.

## Differences from upstream

- Structurally equal object assertions and chained `any`/`unknown` assertions can give different results or message choices.
- Some assertions in arrays and generic-call contexts are skipped too broadly, such as `fn(["hello"] as any)`.
- Generic calls wrapped in `await`, sequence expressions, optional calls or non-null assertions can be classified differently.
- The `length` property of a class expression can have a different inferred type, affecting redundant-assertion detection.
- JSX intrinsic attributes can lack the expected target type for checking a non-null assertion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-type-assertion) documentation (MIT).
