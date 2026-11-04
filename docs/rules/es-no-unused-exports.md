# es-no-unused-exports

Reports exports nothing imports. Remove the `export`, and [`es-no-unused-vars`](es-no-unused-vars.md) then finds what is dead altogether.

| | |
| --- | --- |
| Upstream | none |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | not measured |

## Enable

```dx
import .ts
rule es-no-unused-exports {
  set keepTags ["@public"]
  set ignore ["src/test/**"]
}
```

Only some folders, with a [`files`](../advanced.md#rules) block:

```dx
import .ts
files "src/ui/**" { rule es-no-unused-exports warn }
```

## Examples

Incorrect:

```ts
// helper.ts: other files import helper.ts, but not this name
export const table = [1, 2];
```

Correct:

```ts
// helper.ts: another file has `import { table } from "./helper"`
export const table = [1, 2];

// @public
export function forMods() {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `ignore` | `string[]` | `[]` | Globs of files whose imports don't count, and whose own exports aren't reported, relative to the config. |
| `keepTags` | `string[]` | `[]` | Comment tags that keep an export, such as `@public`. No tag is special by default. |

"Nothing" means nothing in the program of the `tsconfig.json` beside the config and its `references`. **Code outside it, such as mods or other packages using your files, is invisible**: mark what it uses with `keepTags`, or leave those folders out with a `files` block.

Exports are judged by name, so overloads and merged declarations report once, at the first site, and a tag on any site keeps them all. Tags go in any comment directly before the statement, or before one name in `export { /** @public */ a, b }`, and match whole: `@public` doesn't match `@publication`. Only what a module exports is checked, not class members.

These count as a use of a name:

- `import { a }`, `import a`, `import { a as b }`
- `export { a } from "./m"` and `export * from "./m"`, when something uses the name they publish. Reading through such files is followed
- `import { a } from "./m"; export { a }`, whether or not anything uses it from there

These count as a use of every name in the module:

- `import * as ns`, `import x = require()`, `import()`, `import("x")` types and `declare module "./m"`
- an import of a name the module doesn't publish

Files nothing imports at all are entry points, and their exports aren't reported. That covers scripts and files loaded by computed paths. A file only `ignore` files import is not an entry point.

A tag only silences that name's own report: it doesn't count as a use of anything. So `/** @public */ export { a } from "./m"` still leaves `a` in `./m` unused, and needs a tag there too.
