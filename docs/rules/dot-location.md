# dot-location

Keep member-access dots on a consistent side of a line break. The default puts the dot on the same line as the property.

| | |
| --- | --- |
| Upstream | [`@stylistic/dot-location`](https://eslint.style/rules/dot-location) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 91 / 91 corpus cases |

## Enable

```dx
import .ts
rule dot-location
```

## Examples

Incorrect:

```ts
const value = object.
  property;
```

Correct:

```ts
const value = object
  .property;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"object"` / `"property"` | `"property"` | Which side of the dot must share its line. |

```dx
rule dot-location {
  set style "object"
}
```

## Differences from upstream

- The positional style option is named `style`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/dot-location) documentation (MIT).
