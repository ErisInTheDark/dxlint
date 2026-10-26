# es-no-param-reassign

Reports writes to function parameters. It can also report changes to parameter properties, helping avoid unexpected side effects on a caller's objects.

| | |
| --- | --- |
| Upstream | [`no-param-reassign`](https://eslint.org/docs/latest/rules/no-param-reassign) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 78 / 78 corpus cases |

## Enable

```dx
import .ts
rule es-no-param-reassign
```

## Examples

Incorrect:

```ts
function foo(bar) {
  bar = 13;
}
```

Correct:

```ts
function foo(bar) {
  const baz = bar;
  return baz;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `props` | `boolean` | `false` | Also report modifications to parameter properties. |
| `ignorePropertyModificationsFor` | `string[]` | `[]` | Parameter names whose properties may change. |
| `ignorePropertyModificationsForRegex` | `string[]` | `[]` | Regular expressions matching exempt parameter names. |

```dx
rule es-no-param-reassign {
  set props true
  set ignorePropertyModificationsFor ["draft"]
  set ignorePropertyModificationsForRegex ["^mutable"]
}
```

Exemptions apply only to property changes, not reassignment of the parameter itself.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-param-reassign` | Writes to parameter bindings. |
| `es-no-param-reassign:property` | Property writes when `props` is `true`. |

`rule es-no-param-reassign` turns on every variant; `rule es-no-param-reassign:property off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-param-reassign) documentation (MIT).
