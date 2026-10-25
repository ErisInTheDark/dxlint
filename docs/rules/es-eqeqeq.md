# es-eqeqeq

Prefer strict equality operators so comparisons do not silently coerce values. Options can allow selected loose comparisons or change how `null` is treated.

| | |
| --- | --- |
| Upstream | [`es-eqeqeq`](https://eslint.org/docs/latest/rules/eqeqeq) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 77 / 77 corpus cases |

## Enable

```dx
import .ts
rule es-eqeqeq
```

## Examples

Incorrect:

```ts
if (x == 42) {}
if (value != undefined) {}
```

Correct:

```ts
if (x === 42) {}
if (value !== undefined) {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | `("always" \| "smart" \| "allow-null" \| { null: "always" \| "never" \| "ignore" })[]` | `[]` | Positional mode and optional null policy. Empty means `always`. `smart` allows same-type literals, `typeof` and null comparisons; `allow-null` exempts null. |

```dx
rule es-eqeqeq {
  set options ["always", { null: "ignore" }]
}
```

## Differences from upstream

- Put upstream's positional arguments in the `options` array.
- Only safe comparisons receive autofixes: same-type literals other than regexes, and `typeof` compared with a string. Upstream's suggestions for other comparisons are not provided.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/eqeqeq) documentation (MIT).
