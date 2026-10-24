# no-misused-new

Reports constructor-like declarations in the wrong place: a class method named `new`, an interface constructing itself, or an interface/type member named `constructor`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-misused-new`](https://typescript-eslint.io/rules/no-misused-new) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 19 / 19 corpus cases |

## Enable

```dx
import .ts
rule no-misused-new
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
| `no-misused-new` | Construct signatures returning their enclosing interface. |
| `no-misused-new:class` | Class methods named `new`. |
| `no-misused-new:constructor` | Interface and type-literal methods named `constructor`. |

`rule no-misused-new` turns on every variant; `rule no-misused-new:class off` turns one off.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-misused-new) documentation (MIT).
