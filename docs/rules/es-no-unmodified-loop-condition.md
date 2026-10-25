# es-no-unmodified-loop-condition

Reports variables used in a loop condition but never modified in the loop. A forgotten update can leave the loop running forever.

| | |
| --- | --- |
| Upstream | [`es-no-unmodified-loop-condition`](https://eslint.org/docs/latest/rules/no-unmodified-loop-condition) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 40 / 40 corpus cases |

## Enable

```dx
import .ts
rule es-no-unmodified-loop-condition
```

## Examples

Incorrect:

```ts
let node = root;
while (node) {
  visit(node);
}
```

Correct:

```ts
let node = root;
while (node) {
  visit(node);
  node = node.parent;
}
```

Calls and property reads can change without assigning to a local variable, so they are not treated as fixed loop conditions.

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `checkConditionalExpressions` | boolean | `false` | Check branches of ternary conditions separately rather than grouping the whole expression. |

```dx
rule es-no-unmodified-loop-condition {
  set checkConditionalExpressions true
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unmodified-loop-condition) documentation (MIT).
