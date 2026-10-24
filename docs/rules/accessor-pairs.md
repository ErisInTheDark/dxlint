# accessor-pairs

Reports properties with a setter but no getter. A property that can be written but not read is often a mistake; requiring setters for getters is optional.

| | |
| --- | --- |
| Upstream | [`accessor-pairs`](https://eslint.org/docs/latest/rules/accessor-pairs) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 313 / 313 corpus cases |

## Enable

```dx
import .ts
rule accessor-pairs
```

## Examples

Incorrect:

```ts
const obj = {
  set value(next) { this.stored = next; }
};
```

Correct:

```ts
const obj = {
  set value(next) { this.stored = next; },
  get value() { return this.stored; }
};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `getWithoutSet` | `boolean` | `false` | Require setters for getters. |
| `setWithoutGet` | `boolean` | `true` | Require getters for setters. |
| `enforceForClassMembers` | `boolean` | `true` | Check class accessors. |
| `enforceForTSTypes` | `boolean` | `false` | Check accessors in TypeScript types. |

```dx
rule accessor-pairs {
  set getWithoutSet true
}
```

The class and type options apply to the base variant, not property descriptors.

## Variants

| Variant | Checks |
| --- | --- |
| `accessor-pairs` | Object, class and type accessors. |
| `accessor-pairs:descriptor` | Getters and setters in property descriptors. |

`rule accessor-pairs` turns on every variant; `rule accessor-pairs:descriptor off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/accessor-pairs) documentation (MIT).
