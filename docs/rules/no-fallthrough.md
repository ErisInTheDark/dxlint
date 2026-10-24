# no-fallthrough

Reports switch cases that run into the next case without a break or other exit. Mark intentional fallthrough with a comment such as `// falls through`.

| | |
| --- | --- |
| Upstream | [`no-fallthrough`](https://eslint.org/docs/latest/rules/no-fallthrough) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:correctness`, `rules:recommended` |
| Parity | 84 / 87 corpus cases |

## Enable

```dx
import .ts
rule no-fallthrough
```

## Examples

Incorrect:

```ts
switch (value) {
  case 1: first();
  case 2: second();
}
```

Correct:

```ts
switch (value) {
  case 1: first(); break;
  case 2: second();
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowEmptyCase` | `boolean` | `false` | Allow empty cases separated by blank lines. |
| `commentPattern` | `regex` or `null` | `null` | Custom fallthrough comment pattern; null uses `/falls?\s?through/i`. |
| `reportUnusedFallthroughComment` | `boolean` | `false` | Report fallthrough comments on cases that cannot fall through. |

```dx
rule no-fallthrough {
  set reportUnusedFallthroughComment true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-fallthrough` | Unmarked fallthrough into a case. |
| `no-fallthrough:unused-comment` | Unneeded fallthrough comments, when the option is enabled. |

`rule no-fallthrough` turns on every variant; `rule no-fallthrough:unused-comment off` turns one off.

## Differences from upstream

- Three corpus cases depend on ESLint inline configuration or suppression comments. Use dxlint's own configuration and suppression syntax instead.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-fallthrough) documentation (MIT).
