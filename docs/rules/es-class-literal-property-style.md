# es-class-literal-property-style

Enforces one way to expose constant literal values on classes: readonly fields or getters. Objects, arrays and function values are not checked because changing their declaration can change behaviour.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/class-literal-property-style`](https://typescript-eslint.io/rules/class-literal-property-style) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions` |
| Parity | 53 / 53 corpus cases |

## Enable

```dx
import .ts
rule es-class-literal-property-style
```

## Examples

Incorrect:

```ts
class Example {
  get answer() { return 42; }
}
```

Correct:

```ts
class Example {
  readonly answer = 42;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"fields"` or `"getters"` | `"fields"` | Preferred declaration for literal-valued members. |

```dx
rule es-class-literal-property-style {
  set mode "getters"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-class-literal-property-style` | Literal-returning getters when fields are preferred. |
| `es-class-literal-property-style:getters` | Readonly literal fields when getters are preferred. |

`rule es-class-literal-property-style` turns on every variant; `rule es-class-literal-property-style:getters off` turns one off.

## Differences from upstream

- The upstream string option is named `mode` in dxlint.
- dxlint reports the issue without upstream's replacement suggestions.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/class-literal-property-style) documentation (MIT).
