# es-object-curly-spacing

Controls spaces inside object-like braces, including destructuring, named imports and TypeScript type bodies. By default, spaces are forbidden and empty braces are ignored.

| | |
| --- | --- |
| Upstream | [`@stylistic/object-curly-spacing`](https://eslint.style/rules/object-curly-spacing) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule es-object-curly-spacing
```

## Examples

Incorrect:

```ts
const obj = { foo: "bar" };
import { foo } from "bar";
```

Correct:

```ts
const obj = {foo: "bar"};
import {foo} from "bar";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "never"` | `"never"` | Require or forbid inner spaces. |
| `arraysInObjects` | `boolean \| null` | `null` | Override spacing beside array values and index signatures. |
| `objectsInObjects` | `boolean \| null` | `null` | Override spacing beside nested objects. |
| `overrides` | Map from syntax kind to `"always" \| "never"` | `{}` | Override spacing for particular kinds of syntax. |
| `emptyObjects` | `"ignore" \| "always" \| "never"` | `"ignore"` | Spacing inside empty braces. |

```dx
rule es-object-curly-spacing {
  set style "always"
  set emptyObjects "never"
}
```

Override keys follow upstream syntax kinds, such as `ObjectPattern`, `ObjectExpression`, `ImportDeclaration`, `ImportAttributes`, `ExportNamedDeclaration`, `ExportAllDeclaration`, `TSMappedType`, `TSTypeLiteral`, `TSInterfaceBody` and `TSEnumBody`.

## Variants

| Variant | Checks |
| --- | --- |
| `es-object-curly-spacing:open` | Space after an opening brace. |
| `es-object-curly-spacing:close` | Space before a closing brace. |
| `es-object-curly-spacing:empty` | Space inside empty braces. |

`rule es-object-curly-spacing` turns on every variant; `rule es-object-curly-spacing:empty off` turns one off.

## Differences from upstream

- Set the upstream positional string through the named `style` option.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/object-curly-spacing) documentation (MIT).
