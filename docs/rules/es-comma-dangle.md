# es-comma-dangle

Controls trailing commas in lists, including arrays, objects, calls, imports and TypeScript declarations. By default, trailing commas are forbidden.

| | |
| --- | --- |
| Upstream | [`@stylistic/comma-dangle`](https://eslint.style/rules/comma-dangle) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 397 / 407 corpus cases |

## Enable

```dx
import .ts
rule es-comma-dangle
```

## Examples

Incorrect:

```ts
const values = [1, 2,];
const item = { name: 'example', };
```

Correct:

```ts
const values = [1, 2];
const item = { name: 'example' };
```

## Options

`style` is the default option: `rule es-comma-dangle set <value>` sets it.

`trailing_comma_mode` accepts `"never"`, `"always"`, `"always-multiline"`, `"only-multiline"` and `"ignore"`. `always-multiline` requires a comma when the last item and closing delimiter are on different lines; `only-multiline` merely permits it there.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `trailing_comma_mode` | `"never"` | Main policy. |
| `arrays` | `trailing_comma_mode` or `null` | `null` | Array literals and patterns. |
| `objects` | `trailing_comma_mode` or `null` | `null` | Object literals and patterns. |
| `functions` | `trailing_comma_mode` or `null` | `null` | Parameters and arguments. |
| `imports` | `trailing_comma_mode` or `null` | `null` | Named imports. |
| `exports` | `trailing_comma_mode` or `null` | `null` | Named exports. |
| `importAttributes` | `trailing_comma_mode` or `null` | `null` | Import attributes. |
| `dynamicImports` | `trailing_comma_mode` or `null` | `null` | Dynamic import arguments. |
| `enums` | `trailing_comma_mode` or `null` | `null` | Enum members. |
| `generics` | `trailing_comma_mode` or `null` | `null` | Generic parameter lists. |
| `tuples` | `trailing_comma_mode` or `null` | `null` | Tuple elements. |

`null` inherits `style`.

```dx
rule es-comma-dangle {
  set style "always-multiline"
  set functions "never"
}
```

## Differences from upstream

- The upstream string option is named `style`; per-syntax options default to `null` and inherit it.
- Eight corpus cases use unsupported Flow syntax.
- Two corpus cases execute another ESLint plugin rule. dxlint does not run ESLint plugins or their hooks.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/comma-dangle) documentation (MIT).
