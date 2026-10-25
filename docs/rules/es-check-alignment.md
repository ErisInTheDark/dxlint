# es-check-alignment

Reports misaligned JSDoc blocks. Each interior `*` and the closing `*/` should start one column to the right of the opening `/**`; blank lines inside the block need a `*` too.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:format` |

## Enable

```dx
import .ts
rule es-check-alignment
```

## Examples

Incorrect:

```ts
/**
* Describes the function.
*/
function run() {}
```

Correct:

```ts
/**
 * Describes the function.
 */
function run() {}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-check-alignment` | Interior stars. |
| `es-check-alignment:blank` | Blank lines without a star. |
| `es-check-alignment:close` | Closing delimiter indentation. |

`rule es-check-alignment` turns on every variant; `rule es-check-alignment:blank off` turns one off.
