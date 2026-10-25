# es-member-delimiter-style

Reports inconsistent separators between interface and type literal members. Multiline and single-line declarations can use different policies.

| | |
| --- | --- |
| Upstream | [`@stylistic/member-delimiter-style`](https://eslint.style/rules/member-delimiter-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:formatting` |
| Parity | 160 / 160 corpus cases |

## Enable

```dx
import .ts
rule es-member-delimiter-style
```

## Examples

Incorrect:

```ts
interface Example {
  name: string,
  value: number
}
```

Correct:

```ts
interface Example {
  name: string;
  value: number;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `delimiter` | `"semi"`, `"comma"`, `"none"`, or `null` | `null` | Fallback delimiter for legacy configurations. |
| `multiline` | `member_separator_options` | `{ delimiter: null, requireLast: null }` | Multiline policy. |
| `singleline` | `member_separator_options` | `{ delimiter: null, requireLast: null }` | Single-line policy. |
| `overrides` | `{ interface?: member_separator_config; typeLiteral?: member_separator_config }` | `{ interface: null, typeLiteral: null }` | Per-declaration policies. |
| `multilineDetection` | `"brackets"` or `"last-member"` | `"brackets"` | How multiline declarations are classified. |

Separator options accept `delimiter` and `requireLast`. Effective defaults are semicolons, required on the last multiline member but absent from the last single-line member. A separator config accepts `multiline` and `singleline` policies. `"last-member"` treats a declaration as single-line when its last member ends on the closing brace's line.

```dx
rule es-member-delimiter-style {
  set multiline { delimiter: "comma", requireLast: true }
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/member-delimiter-style) documentation (MIT).
