# es-prefer-destructuring

Reports property or array-element reads that can be written as destructuring declarations or assignments. It includes TypeScript-aware handling of annotated declarations and iterable values.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/prefer-destructuring`](https://typescript-eslint.io/rules/prefer-destructuring) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 195 / 195 corpus cases |

## Enable

```dx
import .ts
rule es-prefer-destructuring
```

## Examples

Incorrect:

```ts
const values = ["a"];
const first = values[0];
```

Correct:

```ts
const values = ["a"];
const [first] = values;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of destructuring policy objects | `[]` | Category settings followed by enforcement settings. |

The first object accepts `array` and `object` booleans, or separate `VariableDeclarator` and `AssignmentExpression` objects with those booleans. Both categories are enabled by default. The second object accepts `enforceForRenamedProperties` and `enforceForDeclarationWithTypeAnnotation`, both defaulting to `false`.

```dx
rule es-prefer-destructuring {
  set options [
    { array: true, object: true },
    { enforceForRenamedProperties: true }
  ]
}
```

## Differences from upstream

- This is the TypeScript extension of ESLint's core rule.
- Put upstream positional configuration into the named `options` array.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/prefer-destructuring) documentation (MIT).
