# es-no-unnecessary-type-arguments

Reports explicit type arguments equal to the declared default. Omitting a redundant trailing argument keeps the same type.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-type-arguments`](https://typescript-eslint.io/rules/no-unnecessary-type-arguments) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 71 / 71 corpus cases |

## Enable

```dx
import .ts
rule es-no-unnecessary-type-arguments
```

## Examples

Incorrect:

```ts
function f<T = number>() {}
f<number>();
```

Correct:

```ts
function f<T = number>() {}
f();
f<string>();
```

A type argument counts as the default only when it is the same type: a numeric enum is not `number`, and `A & { x?: T }` is not `A`, though each is assignable to the other. An inferable argument is reported only when it is the only one written, since TypeScript then needs every other one.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-type-arguments) documentation (MIT).
