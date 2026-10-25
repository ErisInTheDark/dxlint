# es-object-shorthand

Controls shorthand properties and methods in object literals. The default prefers concise syntax wherever it can replace a long-form member.

| | |
| --- | --- |
| Upstream | [`es-object-shorthand`](https://eslint.org/docs/latest/rules/object-shorthand) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 295 / 295 corpus cases |

## Enable

```dx
import .ts
rule es-object-shorthand
```

## Examples

Incorrect:

```ts
const foo = { x: x, run: function () {} };
```

Correct:

```ts
const foo = { x, run() {} };
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never" \| "methods" \| "properties" \| "consistent" \| "consistent-as-needed"` | `"always"` | Require shorthand, long form, or consistent property syntax. |
| `avoidQuotes` | `boolean` | `false` | Keep quoted keys in long form. |
| `ignoreConstructors` | `boolean` | `false` | Ignore capitalised method names. |
| `methodsIgnorePattern` | `string` | `""` | Regular expression matching method names to ignore. |
| `avoidExplicitReturnArrows` | `boolean` | `false` | Prefer methods to block-bodied arrows without lexical captures. |

`"methods"` and `"properties"` restrict enforcement to that member kind. `"consistent"` forbids mixing property styles; `"consistent-as-needed"` also requires shorthand when all properties support it.

```dx
rule es-object-shorthand {
  set mode "methods"
  set ignoreConstructors true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-object-shorthand` | Individual properties and methods. |
| `es-object-shorthand:consistent` | Whole-object property consistency. |

`rule es-object-shorthand` turns on every variant; `rule es-object-shorthand:consistent off` turns one off.

## Differences from upstream

- Set the upstream positional string through the named `mode` option.
- Whole-object consistency reports have no autofix; eligible individual members do.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/object-shorthand) documentation (MIT).
