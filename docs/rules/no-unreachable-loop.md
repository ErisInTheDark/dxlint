# no-unreachable-loop

Reports loops whose bodies always exit before a second iteration. Use a conditional instead if at most one pass is intentional.

| | |
| --- | --- |
| Upstream | [`no-unreachable-loop`](https://eslint.org/docs/latest/rules/no-unreachable-loop) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 33 / 33 corpus cases |

## Enable

```dx
import .ts
rule no-unreachable-loop
```

## Examples

Incorrect:

```ts
while (node) {
  visit(node);
  node = node.parent;
  break;
}
```

Correct:

```ts
while (node) {
  visit(node);
  node = node.parent;
}
```

The rule checks exits from the body, not whether the condition or collection allows only one iteration.

## Differences from upstream

- Upstream's `ignore` list of loop types is not available.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-unreachable-loop) documentation (MIT).
