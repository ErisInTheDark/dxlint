# check-alignment

Reports misaligned JSDoc blocks. Each interior `*` and the closing `*/` should start one column to the right of the opening `/**`; blank lines inside the block need a `*` too.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:format` |

## Enable

```dx
import .ts
rule check-alignment
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
| `check-alignment` | Interior stars. |
| `check-alignment:blank` | Blank lines without a star. |
| `check-alignment:close` | Closing delimiter indentation. |

`rule check-alignment` turns on every variant; `rule check-alignment:blank off` turns one off.
