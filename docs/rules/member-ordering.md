# member-ordering

Reports members declared out of order in classes, interfaces, and type literals. The standard order groups signatures, fields, static initialisation, constructors, accessors, and methods.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/member-ordering`](https://typescript-eslint.io/rules/member-ordering) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 372 / 372 corpus cases |

## Enable

```dx
import .ts
rule member-ordering
```

## Examples

Incorrect:

```ts
class Example {
  read() {}
  value = 1;
}
```

Correct:

```ts
class Example {
  value = 1;
  read() {}
}
```

## Options

Set these options on a variant to configure that declaration kind.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `order` | `"alphabetically"`, `"alphabetically-case-insensitive"`, `"as-written"`, `"natural"`, or `"natural-case-insensitive"` | `"as-written"` | Name order within a rank. |
| `optionality` | `"none"`, `"optional-first"`, or `"required-first"` | `"none"` | Order optional and required members. |

```dx
rule member-ordering:interface {
  set order "alphabetically"
  set optionality "required-first"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `member-ordering:class-declaration` | Class declarations. |
| `member-ordering:class-expression` | Class expressions. |
| `member-ordering:interface` | Interfaces. |
| `member-ordering:type-literal` | Type literals. |

`rule member-ordering` turns on every variant; `rule member-ordering:interface off` turns one off.

## Differences from upstream

- Instead of `default`, `classes`, `classExpressions`, `interfaces`, and `typeLiterals` configuration objects, configure the corresponding variants.
- Upstream `optionalityOrder` is named `optionality`.
- Arbitrary `memberTypes` arrays and same-rank groups are not exposed as options. The built-in standard ranking is used.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/member-ordering) documentation (MIT).
