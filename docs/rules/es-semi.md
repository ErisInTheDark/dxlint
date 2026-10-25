# es-semi

Enforces consistent statement-ending semicolons. The default requires semicolons; the alternative omits them where automatic semicolon insertion is safe.

| | |
| --- | --- |
| Upstream | [`@stylistic/semi`](https://eslint.style/rules/semi) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:es-no-semi`, `rules:formatting` |
| Parity | 312 / 313 corpus cases |

## Enable

```dx
import .ts
rule es-semi
```

## Examples

Incorrect:

```ts
const name = "dxlint"
```

Correct:

```ts
const name = "dxlint";
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always" \| "never"` | `"always"` | Require or omit statement semicolons. |
| `omitLastInOneLineBlock` | boolean | `false` | Omit the final semicolon in a single-line block in always mode. |
| `omitLastInOneLineClassBody` | boolean | `false` | Omit the final semicolon in a single-line class body in always mode. |
| `beforeStatementContinuationChars` | `"always" \| "any" \| "never"` | `"any"` | In never mode, control semicolons before lines beginning with continuation characters such as `(` or `[`. Necessary separators remain protected. |

```dx
rule es-semi {
  set mode "never"
  set beforeStatementContinuationChars "always"
}
```

## Semicolon-free style

`import rules:es-no-semi` after `rules:format` switches a project to no semicolons. Lines that would otherwise continue the previous statement keep a guard semicolon at their own start:

```ts
const items = load()
;[1, 2, 3].forEach(console.log)
```

## Differences from upstream

- The upstream string option is named `mode` in dxlint.
- ESLint inline rule configuration and suppression comments are not interpreted. This accounts for the unmatched corpus case.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/semi) documentation (MIT).
