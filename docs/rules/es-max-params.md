# es-max-params

Reports functions with too many parameters. A smaller parameter list can make a function easier to call correctly.

A leading `this: void` parameter is excluded from the count.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/max-params`](https://typescript-eslint.io/rules/max-params) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 82 / 82 corpus cases |

## Enable

```dx
import .ts
rule es-max-params
```

## Examples

Incorrect:

```ts
function example(first: string, second: string, third: string, fourth: string) {}
```

Correct:

```ts
function example(first: string, second: string, third: string) {}
```

## Options

`max` is the default option: `rule es-max-params set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `3` | Maximum parameter count. |

```dx
rule es-max-params {
  set max 2
}
```

## Differences from upstream

- The TypeScript upstream extends ESLint's core rule with `this` parameter handling. dxlint does not expose its `countVoidThis` option or the deprecated core `maximum` alias.
- For a function type such as `(a: number, b: number) => number`, dxlint reports at the function head near `=>`; upstream reports at the opening parameter list. This accounts for one strict-comparison mismatch despite complete semantic parity.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/max-params) documentation (MIT).
