# no-invalid-regexp

Reports invalid patterns and flags passed to `RegExp`. Unlike invalid regex literals, these errors would otherwise appear only when the constructor runs.

| | |
| --- | --- |
| Upstream | [`no-invalid-regexp`](https://eslint.org/docs/latest/rules/no-invalid-regexp) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 112 / 112 corpus cases |

## Enable

```dx
import .ts
rule no-invalid-regexp
```

## Examples

Incorrect:

```ts
RegExp("[");
RegExp(".", "z");
```

Correct:

```ts
RegExp(".");
new RegExp("[a-z]", "i");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowConstructorFlags` | `string[]` | `[]` | Extra case-sensitive flags allowed beyond standard flags. |

```dx
rule no-invalid-regexp {
  set allowConstructorFlags ["a", "z"]
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-invalid-regexp) documentation (MIT).
