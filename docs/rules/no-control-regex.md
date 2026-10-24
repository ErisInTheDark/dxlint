# no-control-regex

Reports raw ASCII control characters and hexadecimal or Unicode escapes for them in regular expressions. Explicit control escapes such as `\t`, `\n` and `\cJ` are allowed.

| | |
| --- | --- |
| Upstream | [`no-control-regex`](https://eslint.org/docs/latest/rules/no-control-regex) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule no-control-regex
```

## Examples

Incorrect:

```ts
const pattern = /\x00/;
const other = /\u000C/;
```

Correct:

```ts
const pattern = /\x20/;
const tab = /\t/;
const newline = new RegExp("\\n");
```

Plain string patterns passed to `RegExp` are checked too. Their decoded contents matter: `new RegExp("\t")` contains a raw tab and is reported, while `new RegExp("\\t")` is allowed.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-control-regex) documentation (MIT).
