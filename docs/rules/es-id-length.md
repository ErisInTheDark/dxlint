# es-id-length

Reports identifier names shorter or longer than the configured limits. Length is counted in graphemes, so a visible character made from several Unicode code points counts as one.

| | |
| --- | --- |
| Upstream | [`es-id-length`](https://eslint.org/docs/latest/rules/id-length) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 181 / 181 corpus cases |

## Enable

```dx
import .ts
rule es-id-length
```

## Examples

Incorrect:

```ts
const x = 5;
function process(e) {}
```

Correct:

```ts
const num = 5;
function process(event) {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `min` | `int` | `2` | Minimum name length. |
| `max` | `int \| null` | `null` | Maximum name length; null means unlimited. |
| `exceptions` | `string[]` | `[]` | Names exempt from both limits. |
| `exceptionPatterns` | `regex[]` | `[]` | Patterns matching exempt names. |

```dx
rule es-id-length {
  set min 3
  set max 30
  set exceptions ["i"]
  set exceptionPatterns [/^[xyz]$/]
}
```

## Differences from upstream

- The upstream `properties` option is not exposed; property definitions and assignments remain checked.
- Use `max null` instead of an infinite maximum, and regex values for `exceptionPatterns`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/id-length) documentation (MIT).
