# es-no-unsafe-optional-chaining

Reports optional chains used where an `undefined` result would cause an error. Parentheses can end a chain's short-circuit protection before a later call or property access.

| | |
| --- | --- |
| Upstream | [`no-unsafe-optional-chaining`](https://eslint.org/docs/latest/rules/no-unsafe-optional-chaining) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 62 / 62 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-optional-chaining
```

## Examples

Incorrect:

```ts
(obj?.foo)();
(obj?.foo).bar;
const { bar } = obj?.foo;
```

Correct:

```ts
(obj?.foo)?.();
obj?.foo?.bar;
const { bar } = obj?.foo ?? {};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `disallowArithmeticOperators` | `boolean` | `false` | Also report arithmetic on optional-chain results, which can produce `NaN`. |

```dx
rule es-no-unsafe-optional-chaining {
  set disallowArithmeticOperators true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unsafe-optional-chaining) documentation (MIT).
