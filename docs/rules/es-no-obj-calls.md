# es-no-obj-calls

Reports calls and constructor calls to built-in objects that are not functions, such as `Math` and `JSON`. Use their properties and methods instead.

| | |
| --- | --- |
| Upstream | [`es-no-obj-calls`](https://eslint.org/docs/latest/rules/no-obj-calls) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 128 / 128 corpus cases |

## Enable

```dx
import .ts
rule es-no-obj-calls
```

## Examples

Incorrect:

```ts
const math = Math();
const json = new JSON();
```

Correct:

```ts
const area = Math.PI * radius * radius;
const json = JSON.parse("{}");
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-obj-calls) documentation (MIT).
