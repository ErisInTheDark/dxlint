# es-no-confusing-void-expression

Reports `void`-typed expressions used where a value appears to be needed. Keep calls that return nothing in statement position rather than storing, passing or returning their results.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-confusing-void-expression`](https://typescript-eslint.io/rules/no-confusing-void-expression) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 109 / 110 corpus cases |

## Enable

```dx
import .ts
rule es-no-confusing-void-expression
```

## Examples

Incorrect:

```ts
const response = alert("Are you sure?");
console.log(alert("Are you sure?"));
```

Correct:

```ts
alert("Hello, world!");
const response = confirm("Are you sure?");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignoreArrowShorthand` | `boolean` | `false` | Allow concise arrow bodies to return void expressions. |
| `ignoreVoidOperator` | `boolean` | `false` | Allow expressions explicitly wrapped with `void`. |
| `ignoreVoidReturningFunctions` | `boolean` | `false` | Allow returns from explicitly or contextually void-returning functions. |

```dx
rule es-no-confusing-void-expression {
  set ignoreVoidOperator true
}
```

Fixes are available for some return expressions and concise arrow bodies, not every report.

## Differences from upstream

- Contextual callback return types from intrinsic JSX attributes can be missed. With `ignoreVoidReturningFunctions` enabled, `<button onClick={() => console.log()} />` can still be reported.
- dxlint exposes automatic fixes but not upstream's separate editor suggestions.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-confusing-void-expression) documentation (MIT).
