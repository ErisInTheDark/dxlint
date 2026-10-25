# es-ban-linter-comment

Reports comments meant for another linter: TSLint and ESLint enable/disable directives and ESLint config comments. Remove them once dxlint lints the project; use `dxlint-ignore` for suppressions.

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

```dx
rule es-ban-linter-comment {
  set formats [{ name: "tslint", pattern: /^(\/\/|\/\*)\s*tslint:/ }]
}
```

## Differences from upstream

- Upstream is `ban-tslint-comment` and reports TSLint directives only; ESLint comments and the `formats` option are dxlint additions.
- Autofixes remove directives when safe. An inline block comment containing a line break is left for manual removal to preserve JavaScript's line-break semantics.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/ban-tslint-comment) documentation (MIT).
