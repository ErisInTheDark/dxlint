# es-arrow-body-style

Controls whether arrow functions use block bodies or return expressions directly. By default, unnecessary blocks around a single returned expression are reported.

| | |
| --- | --- |
| Upstream | [`arrow-body-style`](https://eslint.org/docs/latest/rules/arrow-body-style) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 87 / 87 corpus cases |

## Enable

```dx
import .ts
rule es-arrow-body-style
```

## Examples

Incorrect:

```ts
const answer = () => { return 42; };
```

Correct:

```ts
const answer = () => 42;
const update = () => {
  save();
  return 42;
};
```

## Options

`mode` is the default option: `rule es-arrow-body-style set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"always"`, `"never"` or `"as-needed"` | `"as-needed"` | Require blocks, forbid blocks, or remove unnecessary blocks. |
| `requireReturnForObjectLiteral` | `boolean` | `false` | With `as-needed`, require a block and return for object literals. |

```dx
rule es-arrow-body-style {
  set mode "always"
}
```

## Differences from upstream

- The upstream string option is named `mode` in dxlint.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/arrow-body-style) documentation (MIT).
