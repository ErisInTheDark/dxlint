# no-empty-object-type

Reports empty interfaces and `{}` type literals. In TypeScript, `{}` accepts any non-nullish value, not just objects; use `object` or `unknown` when that better expresses your intent.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-empty-object-type`](https://typescript-eslint.io/rules/no-empty-object-type) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule no-empty-object-type
```

## Examples

Incorrect:

```ts
interface AnyObject {}
type AnyValue = {};
```

Correct:

```ts
type AnyObject = object;
type AnyValue = unknown;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowInterfaces` | `"always"` / `"never"` / `"with-single-extends"` | `"never"` | Allow empty interfaces, optionally only those extending one base. |
| `allowObjectTypes` | `"always"` / `"never"` | `"never"` | Allow empty object type literals. |
| `allowWithName` | `regex` or `null` | `null` | Allow interface and type alias names matching this pattern. |

```dx
rule no-empty-object-type {
  set allowInterfaces "with-single-extends"
  set allowWithName /Props$/
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-empty-object-type:interface` | Empty interfaces with at most one supertype. |
| `no-empty-object-type:object` | Empty object type literals outside intersections. |

`rule no-empty-object-type` turns on every variant; `rule no-empty-object-type:interface off` turns one off.

## Differences from upstream

- `allowWithName` uses a dxlint regex value instead of an ESLint string pattern.
- dxlint provides diagnostics only, not the upstream replacement suggestions or interface-to-type fix.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-empty-object-type) documentation (MIT).
