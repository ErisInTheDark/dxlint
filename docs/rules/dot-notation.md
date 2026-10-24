# dot-notation

Prefer dot notation when a property can be named directly. Optional exceptions allow bracket access to private or protected members and index signatures.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/dot-notation`](https://typescript-eslint.io/rules/dot-notation) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 132 / 132 corpus cases |

## Enable

```dx
import .ts
rule dot-notation
```

## Examples

Incorrect:

```ts
const value = object["property"];
```

Correct:

```ts
const value = object.property;
const other = object["not-an-identifier"];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowKeywords` | `boolean` | `true` | Allow keywords in dot notation; otherwise require brackets for them. |
| `allowPattern` | `string` | `""` | Regex pattern matching names allowed in brackets. |
| `allowPrivateClassPropertyAccess` | `boolean` | `false` | Allow bracket access to private properties. |
| `allowProtectedClassPropertyAccess` | `boolean` | `false` | Allow bracket access to protected properties. |
| `allowIndexSignaturePropertyAccess` | `boolean` | `false` | Allow bracket access through string-like index signatures. |

```dx
rule dot-notation {
  set allowPrivateClassPropertyAccess true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `dot-notation:computed` | Bracket access replaceable with dot notation. |
| `dot-notation:keyword` | Dot access to keywords when `allowKeywords` is false. |

`rule dot-notation` turns on every variant; `rule dot-notation:keyword off` turns one off.

## Differences from upstream

- Follows the TypeScript extension of the core ESLint rule, including its type-aware exceptions.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/dot-notation) documentation (MIT).
