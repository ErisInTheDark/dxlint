# es-no-restricted-types

Reports types forbidden by your configuration. Restrictions apply to type syntax, not use of the matching runtime value.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-restricted-types`](https://typescript-eslint.io/rules/no-restricted-types) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule es-no-restricted-types
```

## Examples

With a restriction on `OldType`:

Incorrect:

```ts
let item: OldType;
```

Correct:

```ts
let item: NewType;
const itemFactory = OldType;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `types` | `{ [key: string]: restricted_type_policy }` | `{}` | Restricted type text and its message/replacement policy. |

Keys can name a type or a specific generic instantiation. Whitespace is ignored when matching. A policy can be `true`, a message string, or a record with `message`, `fixWith` and `suggest`; `false` and `null` disable that entry. Autofix requires `fixWith`.

```dx
rule es-no-restricted-types {
  set types {
    OldType: { message: "Use NewType instead.", fixWith: "NewType" }
  }
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-restricted-types` | Generic instantiations, type keywords, empty tuples and empty object types. |
| `es-no-restricted-types:name` | Referenced type names. |

`rule es-no-restricted-types` turns on every variant; `rule es-no-restricted-types:name off` turns one off.

## Differences from upstream

- The `suggest` field is accepted but does not produce editor suggestions. Use `fixWith` for an automatic replacement.
- Autofixes are withheld when replacing the type would discard comments.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-restricted-types) documentation (MIT).
