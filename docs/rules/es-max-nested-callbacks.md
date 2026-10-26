# es-max-nested-callbacks

Reports callbacks nested beyond the configured limit. Named functions can make deeply nested callback chains easier to follow.

| | |
| --- | --- |
| Upstream | [`max-nested-callbacks`](https://eslint.org/docs/latest/rules/max-nested-callbacks) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 29 / 29 corpus cases |

## Enable

```dx
import .ts
rule es-max-nested-callbacks
```

## Examples

With `max` set to `1`:

Incorrect:

```ts
first(function () {
  second(function () {});
});
```

Correct:

```ts
first(handleFirst);
function handleFirst() {
  second(function () {});
}
```

## Options

`max` is the default option: `rule es-max-nested-callbacks set <value>` sets it.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(0..)` | `10` | Maximum callback nesting. |

```dx
rule es-max-nested-callbacks {
  set max 1
}
```

## Differences from upstream

- `checkConstructorCallCallbacks` and the deprecated `maximum` alias are not exposed.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-nested-callbacks) documentation (MIT).
