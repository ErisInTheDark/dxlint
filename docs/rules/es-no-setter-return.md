# es-no-setter-return

Reports values returned from setters, where the caller cannot use them. A bare `return` is allowed for early exits.

| | |
| --- | --- |
| Upstream | [`es-no-setter-return`](https://eslint.org/docs/latest/rules/no-setter-return) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 164 / 164 corpus cases |

## Enable

```dx
import .ts
rule es-no-setter-return
```

## Examples

Incorrect:

```ts
const foo = {
  set a(value) {
    this.val = value;
    return value;
  }
};
```

Correct:

```ts
const foo = {
  set a(value) {
    if (value < 0) return;
    this.val = value;
  }
};
```

Checks object and class setters, and setters in property descriptors.

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-setter-return` | Return statements with values in setters. |
| `es-no-setter-return:arrow` | Concise arrow bodies used as descriptor setters. |

`rule es-no-setter-return` turns on every variant; `rule es-no-setter-return:arrow off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-setter-return) documentation (MIT).
