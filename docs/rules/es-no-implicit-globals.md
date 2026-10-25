# es-no-implicit-globals

Reports declarations and assignments that pollute script global scope. Top-level declarations in modules are module-scoped instead.

| | |
| --- | --- |
| Upstream | [`es-no-implicit-globals`](https://eslint.org/docs/latest/rules/no-implicit-globals) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 247 / 247 corpus cases |

## Enable

```dx
import .ts
rule es-no-implicit-globals
```

## Examples

In a non-module script:

Incorrect:

```js
var count = 1;
function update() {}
```

Correct:

```js
(function () {
  var count = 1;
  function update() {}
})();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `lexicalBindings` | `boolean` | `false` | Also check top-level `let`, `const`, and class declarations. |

```dx
rule es-no-implicit-globals {
  set lexicalBindings true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `es-no-implicit-globals` | Global declarations in scripts. |
| `es-no-implicit-globals:leak` | Undeclared assignments that leak globals. |
| `es-no-implicit-globals:readonly` | Writes to read-only globals. |

`rule es-no-implicit-globals` turns on every variant; `rule es-no-implicit-globals:leak off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-implicit-globals) documentation (MIT).
