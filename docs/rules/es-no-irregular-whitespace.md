# es-no-irregular-whitespace

Reports unusual whitespace characters that are easy to overlook or confuse with ordinary spaces. String literals and a leading byte-order mark are excluded.

| | |
| --- | --- |
| Upstream | [`es-no-irregular-whitespace`](https://eslint.org/docs/latest/rules/no-irregular-whitespace) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 280 / 280 corpus cases |

## Enable

```dx
import .ts
rule es-no-irregular-whitespace
```

## Examples

The space after `const` in the incorrect example is a literal non-breaking space, U+00A0.

Incorrect:

```ts
const value = 1;
```

Correct:

```ts
const value = 1;
```

## Differences from upstream

- The `skipStrings`, `skipComments`, `skipRegExps`, `skipTemplates`, and `skipJSXText` options are not exposed. Strings are always skipped; comments, regex literals, templates, and JSX text are checked.
- A consecutive run of irregular spaces is reported together.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-irregular-whitespace) documentation (MIT).
