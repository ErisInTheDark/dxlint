# es-consistent-type-exports

Mark exports that exist only in the type system with `type`. This makes their lack of a runtime value clear to readers and transpilers.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/consistent-type-exports`](https://typescript-eslint.io/rules/consistent-type-exports) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 49 / 49 corpus cases |

## Enable

```dx
import .ts
rule es-consistent-type-exports
```

## Examples

Incorrect:

```ts
interface ButtonProps { onClick(): void }
export { ButtonProps };
```

Correct:

```ts
interface ButtonProps { onClick(): void }
export type { ButtonProps };
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `fixMixedExportsWithInlineTypeSpecifier` | `boolean` | `false` | Fix mixed exports with inline `type` specifiers instead of a separate type export. |

```dx
rule es-consistent-type-exports {
  set fixMixedExportsWithInlineTypeSpecifier true
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/consistent-type-exports) documentation (MIT).
