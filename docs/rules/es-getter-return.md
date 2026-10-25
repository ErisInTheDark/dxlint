# es-getter-return

Require getters to return a value on paths that finish normally. A getter with no useful return value often hides a missing `return`.

| | |
| --- | --- |
| Upstream | [`es-getter-return`](https://eslint.org/docs/latest/rules/getter-return) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | 81 / 81 corpus cases |

## Enable

```dx
import .ts
rule es-getter-return
```

## Examples

Incorrect:

```ts
const person = {
  get name() {}
};
```

Correct:

```ts
const person = {
  get name() { return "Ada"; }
};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowImplicit` | `boolean` | `false` | Accept a bare `return;` that returns undefined. |

```dx
rule es-getter-return {
  set allowImplicit true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-getter-return` | Getter paths that finish without returning. |
| `es-getter-return:bare` | Bare return statements in getters. |

`rule es-getter-return` turns on every variant; `rule es-getter-return:bare off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/getter-return) documentation (MIT).
