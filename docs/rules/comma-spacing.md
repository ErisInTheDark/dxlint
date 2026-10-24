# comma-spacing

Enforces spaces around commas. By default, commas have no space before them and a space after them; closing delimiters and array holes have exceptions.

| | |
| --- | --- |
| Upstream | [`@stylistic/comma-spacing`](https://eslint.style/rules/comma-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 185 / 187 corpus cases |

## Enable

```dx
import .ts
rule comma-spacing
```

## Examples

Incorrect:

```ts
const values = [1 ,2];
run(first ,second);
```

Correct:

```ts
const values = [1, 2];
run(first, second);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `before` | `boolean` | `false` | Require spaces before commas; false forbids them. |
| `after` | `boolean` | `true` | Require spaces after commas; false forbids them. |

```dx
rule comma-spacing {
  set after false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `comma-spacing:before` | Spacing before commas. |
| `comma-spacing:after` | Spacing after commas. |

`rule comma-spacing` turns on every variant; `rule comma-spacing:before off` turns one off.

## Differences from upstream

- Two corpus cases contain incomplete declarations such as `let foo,`. dxlint does not recover them as the upstream parser does.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/comma-spacing) documentation (MIT).
