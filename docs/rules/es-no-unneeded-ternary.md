# es-no-unneeded-ternary

Reports ternaries that can be replaced with simpler boolean expressions. An optional check also covers default-value patterns such as `x ? x : fallback`.

| | |
| --- | --- |
| Upstream | [`no-unneeded-ternary`](https://eslint.org/docs/latest/rules/no-unneeded-ternary) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 47 / 47 corpus cases |

## Enable

```dx
import .ts
rule es-no-unneeded-ternary
```

## Examples

Incorrect:

```ts
const isYes = answer === 1 ? true : false;
```

Correct:

```ts
const isYes = answer === 1;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `defaultAssignment` | boolean | `true` | Allow `x ? x : fallback`; owned by `:default-assignment`. |

```dx
rule es-no-unneeded-ternary:default-assignment {
  set defaultAssignment false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unneeded-ternary` | Ternaries selecting boolean literals. |
| `es-no-unneeded-ternary:default-assignment` | Redundant default-value ternaries when the option is false. |

`rule es-no-unneeded-ternary` turns on every variant; `rule es-no-unneeded-ternary:default-assignment off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unneeded-ternary) documentation (MIT).
