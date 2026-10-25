# es-no-var-requires

Reports `require()` calls used to obtain a module value outside an import declaration. Prefer ES imports or TypeScript's `import name = require("module")` form.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-var-requires`](https://typescript-eslint.io/rules/no-var-requires) |
| Needs types | no |
| Fix | none |
| Rulesets | `rules:style` |

## Enable

```dx
import .ts
rule es-no-var-requires
```

## Examples

Incorrect:

```ts
const foo = require("foo");
```

Correct:

```ts
import foo from "foo";
import bar = require("bar");
require("side-effects");
```

## Differences from upstream

- The upstream `allow` path-pattern option is not exposed.
- Upstream has deprecated this rule in favour of `es-no-require-imports`.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-var-requires) documentation (MIT).
