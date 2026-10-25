# es-logical-assignment-operators

Reports assignments that can use `||=`, `&&=`, or `??=`. You can reverse the policy to require the longer form.

| | |
| --- | --- |
| Upstream | [`es-logical-assignment-operators`](https://eslint.org/docs/latest/rules/logical-assignment-operators) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 304 / 304 corpus cases |

## Enable

```dx
import .ts
rule es-logical-assignment-operators
```

## Examples

Incorrect:

```ts
value = value || fallback;
value = value ?? fallback;
```

Correct:

```ts
value ||= fallback;
value ??= fallback;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` or `"never"` | `"always"` | Require or forbid logical assignment shorthand. |
| `enforceForIfStatements` | `boolean` | `false` | Also check equivalent conditional assignments. |

```dx
rule es-logical-assignment-operators {
  set enforceForIfStatements true
}
```

## Differences from upstream

- The upstream string argument is named `mode` in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/logical-assignment-operators) documentation (MIT).
