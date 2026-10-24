# consistent-indexed-object-style

Enforces one form for dictionary-like types: `Record` or an index signature. By default, eligible index signatures and mapped types should use `Record`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-indexed-object-style`](https://typescript-eslint.io/rules/consistent-indexed-object-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 110 / 112 corpus cases |

## Enable

```dx
import .ts
rule consistent-indexed-object-style
```

## Examples

Incorrect:

```ts
type Dictionary = { [key: string]: unknown };
```

Correct:

```ts
type Dictionary = Record<string, unknown>;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"record"` or `"index-signature"` | `"record"` | Preferred indexed object notation. |

```dx
rule consistent-indexed-object-style {
  set mode "index-signature"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `consistent-indexed-object-style` | Index signatures in type literals. |
| `consistent-indexed-object-style:interface` | Index-only interfaces. |
| `consistent-indexed-object-style:mapped` | Eligible mapped types. |
| `consistent-indexed-object-style:record` | `Record` references when index signatures are preferred. |

`rule consistent-indexed-object-style` turns on every variant; `rule consistent-indexed-object-style:mapped off` turns one off.

## Differences from upstream

- The upstream string option is named `mode`.
- Two corpus cases use malformed empty computed members, such as `type Foo = { [] };`. dxlint does not recover these as the upstream parser does.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-indexed-object-style) documentation (MIT).
