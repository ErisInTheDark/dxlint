# no-array-constructor

Reports array constructor calls that can use array literals instead. Single-argument constructors and constructors with explicit type arguments are allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-array-constructor`](https://typescript-eslint.io/rules/no-array-constructor) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 111 / 111 corpus cases |

## Enable

```dx
import .ts
rule no-array-constructor
```

## Examples

Incorrect:

```ts
Array(0, 1, 2);
new Array(0, 1, 2);
```

Correct:

```ts
[0, 1, 2];
Array(500);
new Array<number>(0, 1, 2);
```

The upstream TypeScript rule extends ESLint's core rule with support for typed constructors.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-array-constructor) and [ESLint](https://eslint.org/docs/latest/rules/no-array-constructor) documentation (MIT).
