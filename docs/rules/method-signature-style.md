# method-signature-style

Reports function members written in the wrong signature style. Function properties and shorthand methods have different parameter variance under TypeScript's `strictFunctionTypes`, so changing style can affect type checking.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/method-signature-style`](https://typescript-eslint.io/rules/method-signature-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 62 / 62 corpus cases |

## Enable

```dx
import .ts
rule method-signature-style
```

## Examples

Incorrect:

```ts
interface Example {
  run(value: string): number;
}
```

Correct:

```ts
interface Example {
  run: (value: string) => number;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"method"` or `"property"` | `"property"` | Preferred function member syntax. |

```dx
rule method-signature-style {
  set mode "method"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `method-signature-style` | Method shorthand when property style is selected. |
| `method-signature-style:property` | Function properties when method style is selected. |

`rule method-signature-style` turns on every variant; `rule method-signature-style:property off` turns one off.

## Differences from upstream

- The upstream string argument is named `mode` in dxlint.
- dxlint reports shorthand methods as forbidden when property style is selected, and function properties as forbidden when method style is selected.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/method-signature-style) documentation (MIT).
