# no-require-imports

Reports CommonJS `require()` imports in favour of ES module syntax. This also covers TypeScript `import = require()` unless explicitly allowed.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-require-imports`](https://typescript-eslint.io/rules/no-require-imports) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:recommended`, `rules:suspicious` |
| Parity | 46 / 46 corpus cases |

## Enable

```dx
import .ts
rule no-require-imports
```

## Examples

Incorrect:

```ts
const lib = require("lib");
import other = require("other");
```

Correct:

```ts
import * as lib from "lib";
import * as other from "other";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allow` | `string[]` | `[]` | Regular expressions matching module paths that may be required. |
| `allowAsImport` | `boolean` | `false` | Allow TypeScript `import = require()` declarations. |

```dx
rule no-require-imports {
  set allowAsImport true
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-require-imports) documentation (MIT).
