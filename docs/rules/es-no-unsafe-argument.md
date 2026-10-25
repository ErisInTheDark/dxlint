# es-no-unsafe-argument

Reports `any` values passed to typed parameters. Also checks unsafe array and tuple spreads and generic type arguments.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unsafe-argument`](https://typescript-eslint.io/rules/no-unsafe-argument) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 36 / 45 corpus cases |

## Enable

```dx
import .ts
rule es-no-unsafe-argument
```

## Examples

Incorrect:

```ts
declare function greet(name: string): void;
declare const input: any;
greet(input);
```

Correct:

```ts
declare function greet(name: string): void;
declare const input: string;
greet(input);
```

Passing `any` to `unknown` is allowed: the receiving code must still narrow the value before using it.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-unsafe-argument` | Unsafe argument values. |
| `es-no-unsafe-argument:array` | Spreads of arrays containing `any`. |
| `es-no-unsafe-argument:spread` | Spreads of an `any` value. |
| `es-no-unsafe-argument:tuple` | Unsafe tuple elements in spreads. |

`rule es-no-unsafe-argument` turns on every variant; `rule es-no-unsafe-argument:tuple off` turns one off.

## Differences from upstream

- Parameter targets after fixed or rest spreads can be resolved incorrectly, missing unsafe arguments.
- Tuple spreads do not yet traverse every unsafe element, for example `foo(...(["a", 1 as any] as const))`.
- Recursive tuple aliases can hide the receiving type, causing missed unsafe arguments, including tagged-template cases.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unsafe-argument) documentation (MIT).
