# es-quote-props

Enforces a consistent policy for quoting property names. The default requires quotes; other styles can remove unnecessary quotes or keep names consistent within an object.

| | |
| --- | --- |
| Upstream | [`@stylistic/quote-props`](https://eslint.style/rules/quote-props) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 160 / 160 corpus cases |

## Enable

```dx
import .ts
rule es-quote-props
```

## Examples

Incorrect:

```ts
const object = { foo: 'bar', baz: 42 };
```

Correct:

```ts
const object = { "foo": 'bar', "baz": 42 };
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `style` | `"always" \| "as-needed" \| "consistent" \| "consistent-as-needed"` | `"always"` | Quote all keys, only required keys, all-or-none within a group, or all keys only when one needs quotes. |
| `keywords` | `boolean` | `false` | Require quotes on reserved words in as-needed styles. |
| `unnecessary` | `boolean` | `true` | Report redundant quotes in `"as-needed"` mode. |
| `numbers` | `boolean` | `false` | Require quoted numeric keys in `"as-needed"` mode. |

```dx
rule es-quote-props {
  set style "as-needed"
}
```

## Differences from upstream

- Set the upstream positional style through the named `style` option.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/quote-props) documentation (MIT).
