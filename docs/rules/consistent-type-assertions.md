# consistent-type-assertions

Use one type assertion style throughout the project. Optional literal checks prefer a type annotation over asserting that an object or array has a type.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-type-assertions`](https://typescript-eslint.io/rules/consistent-type-assertions) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 186 / 186 corpus cases |

## Enable

```dx
import .ts
rule consistent-type-assertions
```

## Examples

Incorrect:

```ts
const value = <number>input;
```

Correct:

```ts
const value = input as number;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `assertionStyle` | `"as"` / `"angle-bracket"` / `"never"` | `"as"` | Required assertion syntax, or forbid assertions. |
| `mode` (on `:object` and `:array`) | `"allow"` / `"allow-as-parameter"` / `"never"` | `"allow"` | Permit literal assertions everywhere, only in argument-like contexts, or nowhere. Assertions to `any`, `unknown` and `const` are exempt. |

```dx
rule consistent-type-assertions {
  set assertionStyle "as"
}
rule consistent-type-assertions:object {
  set mode "never"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `consistent-type-assertions` | Assertion syntax. |
| `consistent-type-assertions:object` | Object literal assertions. |
| `consistent-type-assertions:array` | Array literal assertions. |

`rule consistent-type-assertions` turns on every variant; `rule consistent-type-assertions:array off` turns one off.

## Differences from upstream

- Upstream's `objectLiteralTypeAssertions` and `arrayLiteralTypeAssertions` become `mode` on their respective variants.
- Autofixes convert angle-bracket assertions to `as` when safe. Other style changes and literal assertions have no fix; upstream's literal-assertion suggestions are not provided.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-type-assertions) documentation (MIT).
