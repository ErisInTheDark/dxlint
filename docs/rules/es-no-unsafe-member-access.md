# es-no-unsafe-member-access

Reports property access on `any` values and computed keys typed as `any`. Such access bypasses checks on the object and its properties.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-member-access`](https://typescript-eslint.io/rules/no-unsafe-member-access) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 35 / 36 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-member-access
```

## Examples

Incorrect:

```ts
declare const value: any;
value.name;
value["name"];
```

Correct:

```ts
declare const value: { name: string };
value.name;
value["name"];
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowOptionalChaining` | `boolean` | `false` | Allow optional member access on `any`; available on each variant. |

```dx
rule es-no-unsafe-member-access {
  set allowOptionalChaining true
}
```

## Variants

The base check covers dot access.

| Variant | Checks |
| --- | --- |
| `es-no-unsafe-member-access:computed` | Bracket access on an unsafe object. |
| `es-no-unsafe-member-access:key` | Unsafe computed property keys. |

`rule es-no-unsafe-member-access` turns on every variant; `rule es-no-unsafe-member-access:key off` turns one off.

## Differences from upstream

- Object methods using `this` can be classified differently when `noImplicitThis` is disabled; one corpus case differs.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-member-access) documentation (MIT).
