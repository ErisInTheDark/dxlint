# es-max-depth

Reports control-flow blocks nested beyond the limit. Deep nesting can make the path through a function hard to follow.

| | |
| --- | --- |
| Upstream | [`es-max-depth`](https://eslint.org/docs/latest/rules/max-depth) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 28 / 28 corpus cases |

## Enable

```dx
import .ts
rule es-max-depth
```

## Examples

With `max` set to `2`:

Incorrect:

```ts
function run() {
  for (;;) {
    while (ready) {
      if (active) {}
    }
  }
}
```

Correct:

```ts
function run() {
  for (;;) {
    while (ready) {}
  }
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `4` | Maximum control-flow nesting. |

```dx
rule es-max-depth {
  set max 2
}
```

## Differences from upstream

- Use `max`, not the deprecated upstream alias `maximum`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-depth) documentation (MIT).
