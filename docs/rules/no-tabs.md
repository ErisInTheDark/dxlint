# no-tabs

Reports tab characters anywhere in a file, including comments and strings. Indentation tabs can be allowed separately.

| | |
| --- | --- |
| Upstream | [`@stylistic/no-tabs`](https://eslint.style/rules/no-tabs) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 11 / 11 corpus cases |

## Enable

```dx
import .ts
rule no-tabs
```

## Examples

The incorrect example contains a literal tab before `=`, not the two characters `\t`.

Incorrect:

```ts
var a 	= 2;
```

Correct:

```ts
var a = 2;
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowIndentationTabs` | `boolean` | `false` | Allow tabs at the start of a line. |

```dx
rule no-tabs {
  set allowIndentationTabs true
}
```

Adapted from the [ESLint Stylistic](https://eslint.style/rules/no-tabs) documentation (MIT).
