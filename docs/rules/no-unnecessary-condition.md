# no-unnecessary-condition

Reports conditions whose types make the result fixed. Also checks redundant optional chains, nullish fallbacks, comparisons and array predicate callbacks.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-unnecessary-condition`](https://typescript-eslint.io/rules/no-unnecessary-condition) |
| Needs types | yes |
| Fix | none |
| Rulesets | none |
| Parity | 308 / 318 corpus cases |

## Enable

```dx
import .ts
rule no-unnecessary-condition
```

## Examples

Incorrect:

```ts
function head(items: string[]) {
  if (items) {
    return items[0];
  }
}
```

Correct:

```ts
function head(items: string[]) {
  if (items.length) {
    return items[0];
  }
}
```

Types must accurately describe runtime values. A condition that looks redundant to the type system may still protect against bad external data.

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowConstantLoopConditions` | `"always" \| "never" \| "only-allowed-literals"` | `"never"` | Which constant loop tests to allow; owned by `:loop`. |
| `allowRuleToRunWithoutStrictNullChecksIKnowWhatIAmDoing` | boolean | `false` | Suppress the strict-null-check requirement; owned by `:strict`. |

```dx
rule no-unnecessary-condition:loop {
  set allowConstantLoopConditions "always"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-unnecessary-condition` | `if` and ternary tests. |
| `no-unnecessary-condition:callback` | Direct array predicate callback results. |
| `no-unnecessary-condition:callback-return` | Inferred array predicate callback return types. |
| `no-unnecessary-condition:comparison` | Comparisons of known literal values. |
| `no-unnecessary-condition:logical` | Logical expressions and assignments. |
| `no-unnecessary-condition:loop` | Loop conditions. |
| `no-unnecessary-condition:nullish` | Nullish coalescing and assignment. |
| `no-unnecessary-condition:optional` | Optional chains on non-nullish values. |
| `no-unnecessary-condition:overlap` | Comparisons of types with no overlap. |
| `no-unnecessary-condition:predicate` | Type guard and assertion checks; inactive by default. |
| `no-unnecessary-condition:strict` | Missing `strictNullChecks`. |
| `no-unnecessary-condition:truthiness-assertion` | Truthiness assertion calls; inactive by default. |

`rule no-unnecessary-condition` turns on every variant; `rule no-unnecessary-condition:loop off` turns one off.

## Differences from upstream

- Upstream's `checkTypePredicates` option is not available, so predicate and truthiness-assertion checks remain inactive.
- No autofixes or suggestions are exposed for redundant conditions.
- ESLint inline configuration and suppression directives do not use dxlint's configuration contract.
- Enum comparison messages can show the underlying number rather than the enum member name.
- Indexed access through a constrained `keyof` can miss a known truthy result.
- Keys drawn from tuple unions in `for...of` can give incorrect nullish-assignment results.
- Mapped string indexers such as `Lowercase<string>` can yield different optional-chain decisions.
- Repeated indexed reads may lose an earlier guard's narrowing, for example inside `if (items[0]?.name)`.
- With `strictNullChecks` disabled, nullable unions can be normalised differently.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-unnecessary-condition) documentation (MIT).
