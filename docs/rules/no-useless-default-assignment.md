# no-useless-default-assignment

Reports default values that cannot be used because the incoming value cannot be `undefined`, and defaults that merely supply `undefined`. Accurate checking requires TypeScript's `strictNullChecks`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-useless-default-assignment`](https://typescript-eslint.io/rules/no-useless-default-assignment) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 91 / 91 corpus cases |

## Enable

```dx
import .ts
rule no-useless-default-assignment
```

## Examples

Incorrect:

```ts
function read({ value = "" }: { value: string }) {
  return value;
}
```

Correct:

```ts
function read({ value = "" }: { value?: string }) {
  return value;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing` | `boolean` | `false` | On `:strict`, suppress the warning about disabled strict null checks. |

```dx
rule no-useless-default-assignment:strict {
  set allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing true
}
```

Without strict null checks, types can lose `undefined`, so results may be unreliable.

## Variants

The base check reports useless parameter and destructuring defaults.

| Variant | Checks |
| --- | --- |
| `no-useless-default-assignment:strict` | Files with `strictNullChecks` disabled. |

`rule no-useless-default-assignment` turns on every variant; `rule no-useless-default-assignment:strict off` turns one off.

## Differences from upstream

- The strict-null-check option belongs to `:strict`, rather than the base check.
- The strict-null-check warning is placed at source offset zero instead of upstream's synthetic line-zero position.
- Upstream offers removal suggestions; dxlint reports without a suggestion.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-useless-default-assignment) documentation (MIT).
