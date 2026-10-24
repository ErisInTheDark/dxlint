# getter-return

Require getters to return a value on paths that finish normally. A getter with no useful return value often hides a missing `return`.

| | |
| --- | --- |
| Upstream | [`getter-return`](https://eslint.org/docs/latest/rules/getter-return) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 81 / 81 corpus cases |

## Enable

```dx
import .ts
rule getter-return
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
rule getter-return {
  set allowImplicit true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `getter-return` | Getter paths that finish without returning. |
| `getter-return:bare` | Bare return statements in getters. |

`rule getter-return` turns on every variant; `rule getter-return:bare off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/getter-return) documentation (MIT).
