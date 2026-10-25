# es-type-generic-spacing

Keeps spacing around TypeScript generic brackets consistent. By default, generic brackets sit next to their name and the following call parentheses, without spaces just inside the brackets.

| | |
| --- | --- |
| Upstream | [`@stylistic/type-generic-spacing`](https://eslint.style/rules/type-generic-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule es-type-generic-spacing
```

## Examples

Incorrect:

```ts
type Foo <T = true> = T;
const values: Array <number> = [];
const value = callback <string> ();
```

Correct:

```ts
type Foo<T = true> = T;
const values: Array<number> = [];
const value = callback<string>();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | `boolean` | `false` | Require spacing before the opening generic bracket where a name precedes it. |
| `after` | `boolean` | `false` | Require spacing after the closing bracket before a call, constructor call, tagged template or signature parameter list. |

```dx
rule es-type-generic-spacing {
  set before false
  set after false
}
```

These options do not allow spaces just inside the angle brackets.

## Variants

| Variant | Checks |
| --- | --- |
| `es-type-generic-spacing` | Spacing before the opening bracket. |
| `es-type-generic-spacing:after` | Spacing after the closing bracket. |
| `es-type-generic-spacing:inner-close` | Spaces just before the closing bracket. |
| `es-type-generic-spacing:inner-open` | Spaces just after the opening bracket. |

`rule es-type-generic-spacing` turns on every variant; `rule es-type-generic-spacing:inner-open off` turns one off.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/type-generic-spacing) documentation (MIT).
