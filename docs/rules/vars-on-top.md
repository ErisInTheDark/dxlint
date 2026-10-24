# vars-on-top

Requires `var` declarations to appear together at the start of their function, program or class static block. This makes their function-level scope visible instead of leaving declarations after other statements or inside nested blocks.

| | |
| --- | --- |
| Upstream | [`vars-on-top`](https://eslint.org/docs/latest/rules/vars-on-top) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 61 / 61 corpus cases |

## Enable

```dx
import .ts
rule vars-on-top
```

## Examples

Incorrect:

```js
function run() {
  prepare();
  var result;
}
```

Correct:

```js
function run() {
  var result;
  prepare();
}
```

Multiple leading declarations are allowed. Directives such as `"use strict"` and comments may precede them. This rule does not require moving `let` or `const` declarations.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/vars-on-top) documentation (MIT).
