# no-constant-condition

Reports conditions whose truthiness cannot change. Such conditions can hide unreachable branches or unfinished code.

| | |
| --- | --- |
| Upstream | [`no-constant-condition`](https://eslint.org/docs/latest/rules/no-constant-condition) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 307 / 307 corpus cases |

## Enable

```dx
import .ts
rule no-constant-condition
```

## Examples

Incorrect:

```ts
if (false) {
  doSomethingUnfinished();
}
const result = 0 ? a : b;
```

Correct:

```ts
if (x === 0) {
  doSomething();
}
const result = x !== 0 ? a : b;
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-constant-condition` | `if` and conditional-expression tests. |
| `no-constant-condition:loop` | Loop tests, except literal `while (true)` and loops with yielding effects. |

`rule no-constant-condition` turns on every variant; `rule no-constant-condition:loop off` turns loop checking off.

## Differences from upstream

- Upstream's `checkLoops` option is not exposed as a rule option. The loop variant uses the `while (true)` exception by default; disable that variant to allow constant loop tests.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-constant-condition) documentation (MIT).
