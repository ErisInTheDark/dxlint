# require-unicode-regexp

Requires regular expressions to use Unicode mode with a `u` or `v` flag. This gives more consistent handling of Unicode characters and stricter pattern syntax.

| | |
| --- | --- |
| Upstream | [`require-unicode-regexp`](https://eslint.org/docs/latest/rules/require-unicode-regexp) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 79 / 79 corpus cases |

## Enable

```dx
import .ts
rule require-unicode-regexp
```

## Examples

Incorrect:

```ts
const pattern = /aaa/;
const other = new RegExp("bbb", "gi");
```

Correct:

```ts
const pattern = /aaa/u;
const other = new RegExp("bbb", "giv");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `requireFlag` | `"u"`, `"v"` or `null` | `null` | Require one particular flag; `null` accepts either. |

```dx
rule require-unicode-regexp {
  set requireFlag "u"
}
```

Calls whose flags cannot be evaluated statically are ignored.

## Differences from upstream

- dxlint reports the missing or wrong flag without an editor suggestion.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/require-unicode-regexp) documentation (MIT).
