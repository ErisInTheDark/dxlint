# es-no-undef-init

Reports `let` and `var` declarations explicitly initialised to `undefined`. An uninitialised variable already has that value.

| | |
| --- | --- |
| Upstream | [`es-no-undef-init`](https://eslint.org/docs/latest/rules/no-undef-init) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-no-undef-init
```

## Examples

Incorrect:

```ts
let foo = undefined;
var bar = undefined;
```

Correct:

```ts
let foo;
var bar;
```

Removing an initialiser from a repeated `var` declaration or a `var` inside a loop can change behaviour: the assignment may intentionally reset an existing value.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-undef-init) documentation (MIT).
