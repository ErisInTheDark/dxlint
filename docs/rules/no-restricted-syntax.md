# no-restricted-syntax

Reports syntax added to the open `restricted_syntax` group. The group starts empty: enabling the rule alone does not forbid anything.

| | |
| --- | --- |
| Upstream | [`no-restricted-syntax`](https://eslint.org/docs/latest/rules/no-restricted-syntax) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 33 / 34 corpus cases |

## Enable

```dx
import .ts
group restricted_syntax ts_with
rule no-restricted-syntax
```

## Examples

With `ts_with` added to the group:

Incorrect:

```js
with (me) {
  dontMess();
}
```

Correct:

```js
me.dontMess();
```

## Differences from upstream

- ESLint's selector strings and `{ selector, message }` options are not supported. Add syntax layers to `restricted_syntax`; use ordinary `.dx` rules for conditions and custom messages. See [the language reference](../language.md).
- The default message is `This syntax is not allowed.`
- The corpus adapter misses one selector checking a regular expression's flags, such as the `i` in `/a/i`. This is an adapter limitation, not a selector option available in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-restricted-syntax) documentation (MIT).
