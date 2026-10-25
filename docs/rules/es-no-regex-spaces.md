# es-no-regex-spaces

Reports consecutive literal spaces in regular expressions. A counted repetition makes the intended number of spaces easier to see.

| | |
| --- | --- |
| Upstream | [`es-no-regex-spaces`](https://eslint.org/docs/latest/rules/no-regex-spaces) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 73 / 73 corpus cases |

## Enable

```dx
import .ts
rule es-no-regex-spaces
```

## Examples

Incorrect:

```ts
const literal = /foo   bar/;
const constructed = new RegExp("foo   bar");
```

Correct:

```ts
const literal = /foo {3}bar/;
const constructed = new RegExp("foo {3}bar");
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-regex-spaces) documentation (MIT).
