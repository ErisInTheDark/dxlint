# no-undefined

Reports use of the identifier `undefined`. This supports styles that prefer uninitialised variables, `typeof` checks and `void 0` instead.

| | |
| --- | --- |
| Upstream | [`no-undefined`](https://eslint.org/docs/latest/rules/no-undefined) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 53 / 53 corpus cases |

## Enable

```dx
import .ts
rule no-undefined
```

## Examples

Incorrect:

```ts
const value = undefined;
```

Correct:

```ts
const value = void 0;
```

Unlike a keyword, `undefined` can be shadowed by a local binding. If you only want to prevent shadowing or assignment, use the relevant restricted-name rules instead.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-undefined) documentation (MIT).
