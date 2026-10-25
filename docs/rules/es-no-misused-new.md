# es-no-misused-new

Reports constructor-like declarations in the wrong place: a class method named `new`, an interface constructing itself, or an interface/type member named `constructor`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-misused-new`](https://typescript-eslint.io/rules/no-misused-new) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:suspicious`, `rules:recommended` |
| Parity | 19 / 19 corpus cases |

## Enable

```dx
import .ts
rule es-no-misused-new
```

## Examples

Incorrect:

```ts
declare class C {
  new(): C;
}
interface I {
  new (): I;
  constructor(): void;
}
```

Correct:

```ts
declare class C {
  constructor();
}
interface I {
  new (): C;
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-misused-new` | Construct signatures returning their enclosing interface. |
| `es-no-misused-new:class` | Class methods named `new`. |
| `es-no-misused-new:constructor` | Interface and type-literal methods named `constructor`. |

`rule es-no-misused-new` turns on every variant; `rule es-no-misused-new:class off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-misused-new) documentation (MIT).
