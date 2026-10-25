# es-consistent-return

Requires return statements in a function to consistently return a value or omit it. It also reports reaching the end of a function after another path returned a value, with TypeScript handling for void-returning functions.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-return`](https://typescript-eslint.io/rules/consistent-return) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 77 / 77 corpus cases |

## Enable

```dx
import .ts
rule es-consistent-return
```

## Examples

Incorrect:

```ts
function choose(flag: boolean) {
  if (flag) return 42;
  return;
}
```

Correct:

```ts
function choose(flag: boolean) {
  if (flag) return 42;
  return 0;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `treatUndefinedAsUnspecified` | `boolean` | `false` | Treat returned `undefined` and `void` expressions like bare returns. |

```dx
rule es-consistent-return {
  set treatUndefinedAsUnspecified true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-consistent-return` | Inconsistent explicit returns. |
| `es-consistent-return:fallthrough` | Paths reaching the end without a value. |

`rule es-consistent-return` turns on every variant; `rule es-consistent-return:fallthrough off` turns one off.

## Differences from upstream

- This covers both ESLint's core rule and its TypeScript extension for `void` and `Promise<void>` returns.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-return) documentation (MIT).
