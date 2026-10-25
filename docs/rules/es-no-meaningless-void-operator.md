# es-no-meaningless-void-operator

Reports `void` when it does not discard a useful call result. This catches calls that have changed to return nothing and non-call expressions where `void` adds no useful intent.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-meaningless-void-operator`](https://typescript-eslint.io/rules/no-meaningless-void-operator) |
| Needs types | yes |
| Fix | autofix |
| Rulesets | none |
| Parity | 41 / 43 corpus cases |

## Enable

```dx
import .ts
rule es-no-meaningless-void-operator
```

## Examples

Incorrect:

```ts
function foo() {}
void foo();
```

Correct:

```ts
function foo() {}
foo();
function bar(x: number) { return x * 2; }
void bar(1);
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `checkNever` | `boolean` | `false` | Also report calls returning `never`. |

```dx
rule es-no-meaningless-void-operator {
  set checkNever true
}
```

## Differences from upstream

- dxlint automatically removes `void` in some cases where upstream only offers a suggestion or no automatic fix, including non-call values and `never` results. The two differing corpus cases concern this fix policy.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-meaningless-void-operator) documentation (MIT).
