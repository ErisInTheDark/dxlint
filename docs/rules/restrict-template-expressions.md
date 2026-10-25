# restrict-template-expressions

Checks values interpolated into template strings to avoid surprising text such as `[object Object]`. Strings are accepted, and options allow other types. Tagged templates are not checked.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/restrict-template-expressions`](https://typescript-eslint.io/rules/restrict-template-expressions) |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:types` |
| Parity | 86 / 86 corpus cases |

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
- An unannotated empty array that is later pushed to stays `unknown[]` instead of evolving to its pushed element type, so with `allowArray` it can be reported where upstream accepts it.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/restrict-template-expressions) documentation (MIT).
