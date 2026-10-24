# no-self-assign

Reports assignments that copy a value back onto itself. These usually indicate a typo or unfinished refactor.

| | |
| --- | --- |
| Upstream | [`no-self-assign`](https://eslint.org/docs/latest/rules/no-self-assign) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 85 / 85 corpus cases |

## Enable

```dx
import .ts
rule no-self-assign
```

## Examples

Incorrect:

```ts
foo = foo;
[a, b] = [a, b];
obj.a = obj.a;
```

Correct:

```ts
foo = bar;
[a, b] = [b, a];
obj.a = obj.b;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `props` | `boolean` | `true` | Also check assignments to properties. |

```dx
rule no-self-assign {
  set props false
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-self-assign) documentation (MIT).
