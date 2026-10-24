# prefer-literal-enum-member

Requires explicit enum values to be string or number literals. This avoids surprising references to variables or other enum members.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-literal-enum-member`](https://typescript-eslint.io/rules/prefer-literal-enum-member) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule prefer-literal-enum-member
```

## Examples

Incorrect:

```ts
enum Invalid {
  A = 2 + 2,
  B = A,
}
```

Correct:

```ts
enum Valid {
  A,
  B = 'Test',
  C = 4,
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowBitwiseExpressions` | `boolean` | `false` | Allow bitwise expressions using literals and enum members. |

```dx
rule prefer-literal-enum-member {
  set allowBitwiseExpressions true
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-literal-enum-member) documentation (MIT).
