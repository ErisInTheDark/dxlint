# es-prefer-includes

Reports membership checks written as `indexOf` comparisons or simple regular expression tests. `includes` says directly that a string or array contains a value.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-includes`](https://typescript-eslint.io/rules/prefer-includes) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | `rules:types` |
| Parity | 41 / 42 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-includes
```

## Examples

Incorrect:

```ts
const text = 'example';
text.indexOf('amp') !== -1;
/amp/.test(text);
```

Correct:

```ts
const text = 'example';
text.includes('amp');
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-prefer-includes` | `indexOf` comparisons. |
| `es-prefer-includes:regex` | Simple regular expression tests. |

`rule es-prefer-includes` turns on every variant; `rule es-prefer-includes:regex off` turns one off.

## Differences from upstream

- Some generic parameters constrained to unions of arrays and typed arrays are not recognised as having compatible `indexOf` and `includes` methods.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-includes) documentation (MIT).
