# no-sequences

Reports comma operators that hide multiple operations in one expression. Loop initialisers and updates are allowed; explicitly grouped sequences are allowed by default.

| | |
| --- | --- |
| Upstream | [`no-sequences`](https://eslint.org/docs/latest/rules/no-sequences) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious` |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule no-sequences
```

## Examples

Incorrect:

```ts
foo = doSomething(), val;
if (doSomething(), test) {
  work();
}
```

Correct:

```ts
foo = (doSomething(), val);
for (i = 0, j = 10; i < j; i++, j--) {
  work();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowInParentheses` | `boolean` | `true` | Allow explicitly parenthesised sequences. |

```dx
rule no-sequences {
  set allowInParentheses false
}
```

For concise arrow bodies, an extra pair of parentheses marks a sequence as intentional: `value => ((log(value), value))`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-sequences) documentation (MIT).
