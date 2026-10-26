# es-multiline-comment-style

Reports multiline comments that do not follow the selected shape. Choose starred blocks, bare blocks, or consecutive line comments.

| | |
| --- | --- |
| Upstream | [`@stylistic/multiline-comment-style`](https://eslint.style/rules/multiline-comment-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 126 / 132 corpus cases |

## Enable

```dx
import .ts
rule es-multiline-comment-style
```

## Examples

Incorrect:

```ts
// first line
// second line
const value = 1;
```

Correct:

```ts
/*
 * first line
 * second line
 */
const value = 1;
```

## Options

`mode` is the default option: `rule es-multiline-comment-style set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"starred-block"`, `"bare-block"`, or `"separate-lines"` | `"starred-block"` | Preferred multiline comment shape. |
| `checkJSDoc` | `boolean` | `false` | Include JSDoc when requiring line comments. |
| `checkExclamation` | `boolean` | `false` | Include `/*!` comments when requiring line comments. |

```dx
rule es-multiline-comment-style {
  set mode "separate-lines"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-multiline-comment-style:alignment` | Leading stars and their alignment. |
| `es-multiline-comment-style:close` | Line break before the closing delimiter. |
| `es-multiline-comment-style:consecutive` | Consecutive line comments where a block is required. |
| `es-multiline-comment-style:open` | Line break after the opening delimiter. |
| `es-multiline-comment-style:shape` | Blocks where line comments are required. |

`rule es-multiline-comment-style` turns on every variant; `rule es-multiline-comment-style:alignment off` turns one off.

## Differences from upstream

- The upstream string argument is named `mode`.
- Six corpus cases depend on ESLint inline configuration or suppression directives. dxlint uses its own configuration and suppression contract.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/multiline-comment-style) documentation (MIT).
