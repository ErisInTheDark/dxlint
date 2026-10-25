# es-default-case

Require a `default` clause in switch statements, or a comment explaining its absence. This makes deliberate omissions visible.

| | |
| --- | --- |
| Upstream | [`es-default-case`](https://eslint.org/docs/latest/rules/default-case) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 23 / 23 corpus cases |

## Enable

```dx
import .ts
rule es-default-case
```

## Examples

Incorrect:

```ts
switch (value) {
  case 1: run(); break;
}
```

Correct:

```ts
switch (value) {
  case 1: run(); break;
  default: break;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `commentPattern` | `regex` / `null` | `null` | Custom comment pattern allowing a missing default; otherwise accept `no default`, ignoring case. |

```dx
rule es-default-case {
  set commentPattern /^skip\sdefault/
}
```

## Differences from upstream

- `commentPattern` is a dxlint regex rather than a string containing a regex.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/default-case) documentation (MIT).
