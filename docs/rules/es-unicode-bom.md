# es-unicode-bom

Controls whether a file starts with the Unicode byte order mark, U+FEFF. The default forbids it; UTF-8 files do not need a byte order mark.

| | |
| --- | --- |
| Upstream | [`es-unicode-bom`](https://eslint.org/docs/latest/rules/unicode-bom) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 7 / 7 corpus cases |

## Enable

```dx
import .ts
rule es-unicode-bom
```

## Examples

Incorrect (the next line begins with an invisible U+FEFF):

```js
﻿let value;
```

Correct (no U+FEFF):

```js
let value;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"` or `"never"` | `"never"` | Require or forbid a leading byte order mark. |

```dx
rule es-unicode-bom {
  set mode "always"
}
```

## Differences from upstream

- Set `mode` instead of passing ESLint's string option.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/unicode-bom) documentation (MIT).
