# es-lines-between-class-members

Reports missing or unwanted blank lines between class members. It does not require padding before the first or after the last member.

| | |
| --- | --- |
| Upstream | [`@stylistic/lines-between-class-members`](https://eslint.style/rules/lines-between-class-members) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 125 / 125 corpus cases |

## Enable

```dx
import .ts
rule es-lines-between-class-members
```

## Examples

Incorrect:

```ts
class Example {
  value = 1;
  read() {}
}
```

Correct:

```ts
class Example {
  value = 1;

  read() {}
}
```

## Options

`style` is the default option: `rule es-lines-between-class-members set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"always"` | Require or forbid blank lines. |
| `enforce` | `{ blankLine: "always" or "never"; prev: "method" or "field" or "*"; next: "method" or "field" or "*" }[]` | `[]` | Policies for neighbouring kinds. |
| `exceptAfterSingleLine` | `boolean` | `false` | Skip after single-line members. |
| `exceptAfterOverload` | `boolean` | `true` | Skip after overload signatures. |

```dx
rule es-lines-between-class-members {
  set exceptAfterSingleLine true
}
```

## Differences from upstream

- The upstream string argument is named `style`; member-pair policies use `set enforce`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/lines-between-class-members) documentation (MIT).
