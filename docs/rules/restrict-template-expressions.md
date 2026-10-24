# restrict-template-expressions

Checks values interpolated into template strings to avoid surprising text such as `[object Object]`. Strings are accepted, and options allow other types. Tagged templates are not checked.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/restrict-template-expressions`](https://typescript-eslint.io/rules/restrict-template-expressions) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 84 / 86 corpus cases |

## Enable

```dx
import .ts
rule restrict-template-expressions
```

## Examples

Incorrect:

```ts
const arg = { name: "Foo" };
const message = `arg = ${arg}`;
```

Correct:

```ts
const arg = "foo";
const message = `arg = ${arg}`;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAny` | `boolean` | `true` | Allow `any`. |
| `allowArray` | `boolean` | `false` | Allow arrays when their elements are also allowed. |
| `allowBoolean` | `boolean` | `true` | Allow booleans. |
| `allowNullish` | `boolean` | `true` | Allow `null` and `undefined`. |
| `allowNumber` | `boolean` | `true` | Allow numbers and bigints. |
| `allowRegExp` | `boolean` | `true` | Allow regular expressions. |
| `allowNever` | `boolean` | `false` | Allow `never`. |
| `allow` | `string[]` or `null` | `null` | Allowed type/value names; `null` keeps library `Error`, `URL` and `URLSearchParams`. |

```dx
rule restrict-template-expressions {
  set allowNumber false
  set allow ["Error", "URL"]
}
```

## Differences from upstream

- `allow` takes names rather than upstream's `TypeOrValueSpecifier` objects. It cannot restrict a name to a package, file or library origin.
- With `allowArray` enabled, an unannotated empty local array can be inferred as `unknown[]` and reported. Upstream treats the evolving empty array as `any[]` and accepts it when `allowAny` is enabled. This also affects expressions such as `${arg || "default"}`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/restrict-template-expressions) documentation (MIT).
