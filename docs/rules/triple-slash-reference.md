# triple-slash-reference

Controls TypeScript triple-slash references, preferring imports where possible. Only directives before the first code token count as references.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/triple-slash-reference`](https://typescript-eslint.io/rules/triple-slash-reference) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:conventions`, `rules:recommended` |
| Parity | 21 / 21 corpus cases |

## Enable

```dx
import .ts
rule triple-slash-reference
```

## Examples

Incorrect:

```ts
/// <reference path="code" />
globalThis.value;
```

Correct:

```ts
import { value } from "code";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `lib` | `"always" \| "never"` | `"always"` | Allow or ban library references. |
| `path` | `"always" \| "never"` | `"never"` | Allow or ban path references. |
| `types` | `"always" \| "never" \| "prefer-import"` | `"prefer-import"` | Allow, ban, or report type references only when the same module is already imported. |

```dx
rule triple-slash-reference {
  set path "never"
  set types "never"
}
```

Here, `"always"` permits a directive; it does not require one.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/triple-slash-reference) documentation (MIT).
