# es-lines-around-comment

Reports missing blank lines around standalone comments. Comments beside code and the boundaries of a file do not require extra lines.

| | |
| --- | --- |
| Upstream | [`@stylistic/lines-around-comment`](https://eslint.style/rules/lines-around-comment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 261 / 261 corpus cases |

## Enable

```dx
import .ts
rule es-lines-around-comment
```

## Examples

Incorrect:

```ts
const first = 1;
/* next value */
const second = 2;
```

Correct:

```ts
const first = 1;

/* next value */
const second = 2;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `beforeBlockComment` | `boolean` | `true` | Blank line before block comments. |
| `afterBlockComment` | `boolean` | `false` | Blank line after block comments. |
| `beforeLineComment` | `boolean` | `false` | Blank line before line comments. |
| `afterLineComment` | `boolean` | `false` | Blank line after line comments. |
| `afterHashbangComment` | `boolean` | `false` | Blank line after hashbangs. |
| `applyDefaultIgnorePatterns` | `boolean` | `true` | Exempt common directive comments. |
| `ignorePattern` | `string` | `""` | Exempt matching comment bodies. |
| `allowBlockStart` | `boolean` | `false` | Exempt comments at block starts. |
| `allowBlockEnd` | `boolean` | `false` | Exempt comments at block ends. |
| `allowClassStart` | `boolean` or `null` | `null` | Class-start override; otherwise use block policy. |
| `allowClassEnd` | `boolean` or `null` | `null` | Class-end override; otherwise use block policy. |
| `allowObjectStart` | `boolean` | `false` | Exempt object starts. |
| `allowObjectEnd` | `boolean` | `false` | Exempt object ends. |
| `allowArrayStart` | `boolean` | `false` | Exempt array starts. |
| `allowArrayEnd` | `boolean` | `false` | Exempt array ends. |
| `allowInterfaceStart` | `boolean` | `false` | Exempt interface starts. |
| `allowInterfaceEnd` | `boolean` | `false` | Exempt interface ends. |
| `allowTypeStart` | `boolean` | `false` | Exempt type literal starts. |
| `allowTypeEnd` | `boolean` | `false` | Exempt type literal ends. |
| `allowEnumStart` | `boolean` | `false` | Exempt enum starts. |
| `allowEnumEnd` | `boolean` | `false` | Exempt enum ends. |
| `allowModuleStart` | `boolean` | `false` | Exempt module starts. |
| `allowModuleEnd` | `boolean` | `false` | Exempt module ends. |

```dx
rule es-lines-around-comment {
  set beforeLineComment true
  set allowBlockStart true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-lines-around-comment:before` | Missing lines before comments. |
| `es-lines-around-comment:after` | Missing lines after comments. |

`rule es-lines-around-comment` turns on every variant; `rule es-lines-around-comment:after off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/lines-around-comment) documentation (MIT).
