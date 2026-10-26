# es-ban-linter-comment

Reports comments meant for another linter: TSLint and ESLint enable/disable directives and ESLint config comments. The fix turns ESLint suppressions into `dxlint-ignore` comments for the same rules, and removes the rest.

To convert a project's ESLint suppressions without applying other fixes, run `dxlint -c migrate.dx --fix` with a `migrate.dx` holding only `import .ts` and `rule es-ban-linter-comment`.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/ban-tslint-comment`](https://typescript-eslint.io/rules/ban-tslint-comment) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:style` |
| Parity | 13 / 13 corpus cases |

## Enable

```dx
import .ts
rule es-ban-linter-comment
```

## Examples

Incorrect:

```ts
/* tslint:disable */
// eslint-disable-next-line no-console
/* global window */
someCode();
```

Correct:

```ts
// This comment mentions tslint and eslint without being a directive.
// dxlint-ignore es-no-console
someCode();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `formats` | `{ name: string; pattern: regex }[]` | TSLint directives; ESLint directives; ESLint block config comments (`eslint`, `eslint-env`, `global`, `globals`, `exported`) | Comments to report. `pattern` matches the whole comment, `//` or `/* */` included; `name` goes in the message. |
| `renames` | `{ [rule: string]: string }` | `{}` | dxlint names for ESLint rule ids in converted suppressions, ahead of the built-in mapping. |

```dx
rule es-ban-linter-comment {
  set formats [{ name: "tslint", pattern: /^(\/\/|\/\*)\s*tslint:/ }]
  set renames { "local/condition-complexity": "condition-complexity" }
}
```

Conversions:

| ESLint | dxlint |
| --- | --- |
| `// eslint-disable-next-line a, b -- why` (alone on its line) | `// dxlint-ignore es-a es-b` |
| `code(); // eslint-disable-line a` | `code(); // dxlint-ignore es-a` |
| `/* eslint-disable a */` … `/* eslint-enable a */` | `/* dxlint-ignore-start es-a */` … `/* dxlint-ignore-end es-a */` |
| `// eslint-disable-line` alone on its line | removed (it suppresses nothing) |
| `code(); // eslint-disable-next-line a` | reported, not fixed (no equivalent) |

Rule ids lose their plugin (`@typescript-eslint/`, `@stylistic/`, `import/`, …) and gain `es-`; `eol-last`, `linebreak-style` and `no-trailing-spaces` gain `shared-`; `import/no-deprecated` is `es-import-no-deprecated` and `license-header/header` is `es-header`.

## Differences from upstream

- Upstream is `ban-tslint-comment` and reports TSLint directives only; ESLint comments, their conversion and the `formats`/`renames` options are dxlint additions.
- Autofixes remove other directives when safe. An inline block comment containing a line break is left for manual removal to preserve JavaScript's line-break semantics.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/ban-tslint-comment) documentation (MIT).
