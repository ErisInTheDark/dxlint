# no-div-regex

Reports regular expression literals starting with `=`. The opening `/=` can look like division assignment; a character class makes the intent clear.

| | |
| --- | --- |
| Upstream | [`no-div-regex`](https://eslint.org/docs/latest/rules/no-div-regex) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 3 / 3 corpus cases |

## Enable

```dx
import .ts
rule no-div-regex
```

## Examples

Incorrect:

```ts
function pattern() { return /=foo/; }
```

Correct:

```ts
function pattern() { return /[=]foo/; }
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-div-regex) documentation (MIT).
