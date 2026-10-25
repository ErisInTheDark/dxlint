# es-no-redeclare

Reports names declared more than once in the same scope. TypeScript overloads and supported declaration merges are allowed by default.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-redeclare`](https://typescript-eslint.io/rules/no-redeclare) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 126 / 127 corpus cases |

## Enable

```dx
import .ts
rule es-no-redeclare
```

## Examples

Incorrect:

```ts
var count = 1;
var count = 2;
```

Correct:

```ts
var count = 1;
count = 2;

interface Item { name: string; }
interface Item { id: number; }
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `builtinGlobals` | `boolean` | `true` | Report redeclarations of built-in globals. |
| `ignoreDeclarationMerge` | `boolean` | `true` | Allow supported TypeScript declaration merges. |

```dx
rule es-no-redeclare {
  set ignoreDeclarationMerge false
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-redeclare` | Declaration names in source. |
| `es-no-redeclare:global` | Global declaration mentions and collisions. |

`rule es-no-redeclare` turns on every variant; `rule es-no-redeclare:global off` turns one off.

## Differences from upstream

- The TypeScript upstream rule extends ESLint's core rule.
- Some library types are missing from the built-in redeclaration check. The corpus includes a missed `type NodeListOf = 1;` declaration.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-redeclare) documentation (MIT).
