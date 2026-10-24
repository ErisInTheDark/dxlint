# new-parens

Reports missing or unwanted empty parentheses in constructor calls. Calls with arguments are not changed by this style choice.

| | |
| --- | --- |
| Upstream | [`@stylistic/new-parens`](https://eslint.style/rules/new-parens) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:format` |
| Parity | 42 / 42 corpus cases |

## Enable

```dx
import .ts
rule new-parens
```

## Examples

Incorrect:

```ts
const person = new Person;
```

Correct:

```ts
const person = new Person();
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always"` or `"never"` | `"always"` | Require or forbid empty argument parentheses. |

```dx
rule new-parens {
  set style "never"
}
```

## Differences from upstream

- The upstream string argument is named `style`.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/new-parens) documentation (MIT).
