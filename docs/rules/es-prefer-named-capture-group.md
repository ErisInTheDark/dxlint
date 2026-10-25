# es-prefer-named-capture-group

Reports unnamed capturing groups in regular expressions. Use a named group when its result matters, or a non-capturing group when only grouping is needed.

| | |
| --- | --- |
| Upstream | [`es-prefer-named-capture-group`](https://eslint.org/docs/latest/rules/prefer-named-capture-group) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 57 / 57 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-named-capture-group
```

## Examples

Incorrect:

```ts
const pattern = /(ba[rz])/;
```

Correct:

```ts
const pattern = /(?<id>ba[rz])/;
const flower = /(?:cauli|sun)flower/;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/prefer-named-capture-group) documentation (MIT).
