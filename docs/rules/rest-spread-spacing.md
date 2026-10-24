# rest-spread-spacing

Checks spacing between `...` and a rest or spread expression. By default, no whitespace is allowed there.

| | |
| --- | --- |
| Upstream | [`@stylistic/rest-spread-spacing`](https://eslint.style/rules/rest-spread-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 88 / 88 corpus cases |

## Enable

```dx
import .ts
rule rest-spread-spacing
```

## Examples

Incorrect:

```ts
fn(... args);
const copy = [... items];
function collect(... args) { return args; }
```

Correct:

```ts
fn(...args);
const copy = [...items];
function collect(...args) { return args; }
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"never"` | Require or forbid spacing after `...`. |

```dx
rule rest-spread-spacing {
  set style "always"
}
```

## Differences from upstream

- The upstream string option is named `style` in dxlint.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/rest-spread-spacing) documentation (MIT).
