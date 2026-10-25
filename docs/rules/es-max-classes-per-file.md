# es-max-classes-per-file

Reports files containing more classes than the configured limit. This can help keep a file focused on one responsibility.

| | |
| --- | --- |
| Upstream | [`es-max-classes-per-file`](https://eslint.org/docs/latest/rules/max-classes-per-file) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 18 / 19 corpus cases |

## Enable

```dx
import .ts
rule es-max-classes-per-file
```

## Examples

Incorrect:

```ts
class Foo {}
class Bar {}
```

Correct:

```ts
class Foo {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `max` | `int(1..)` | `1` | Maximum class count. |
| `ignoreExpressions` | `boolean` | `false` | Count declarations only. |

```dx
rule es-max-classes-per-file {
  set max 2
  set ignoreExpressions true
}
```

## Differences from upstream

- One corpus case relies on an ESLint suppression directive. dxlint uses its own configuration and suppression contract.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/max-classes-per-file) documentation (MIT).
