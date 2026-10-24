# consistent-generic-constructors

Enforces one location for generic arguments when constructing a value: the type annotation or the constructor call. By default, arguments belong on the constructor.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-generic-constructors`](https://typescript-eslint.io/rules/consistent-generic-constructors) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 81 / 81 corpus cases |

## Enable

```dx
import .ts
rule consistent-generic-constructors
```

## Examples

Incorrect:

```ts
const map: Map<string, number> = new Map();
```

Correct:

```ts
const map = new Map<string, number>();
```

The rule leaves declarations alone when both sides have type arguments, neither side has them, or the type and constructor names differ.

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"constructor"` or `"type-annotation"` | `"constructor"` | Preferred side for type arguments. |

```dx
rule consistent-generic-constructors {
  set mode "type-annotation"
}
```

## Differences from upstream

- The upstream string option is named `mode` in dxlint.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-generic-constructors) documentation (MIT).
