# no-implicit-globals

Reports declarations and assignments that pollute script global scope. Top-level declarations in modules are module-scoped instead.

| | |
| --- | --- |
| Upstream | [`no-implicit-globals`](https://eslint.org/docs/latest/rules/no-implicit-globals) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 247 / 247 corpus cases |

## Enable

```dx
import .ts
rule no-implicit-globals
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
rule no-implicit-globals {
  set lexicalBindings true
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-implicit-globals` | Global declarations in scripts. |
| `no-implicit-globals:leak` | Undeclared assignments that leak globals. |
| `no-implicit-globals:readonly` | Writes to read-only globals. |

`rule no-implicit-globals` turns on every variant; `rule no-implicit-globals:leak off` turns one off.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-implicit-globals) documentation (MIT).
