# es-no-invalid-void-type

Reports `void` types outside supported return and generic type positions. Use `undefined` when describing an actual value rather than a return value that should be ignored.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/no-invalid-void-type`](https://typescript-eslint.io/rules/no-invalid-void-type) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 96 / 96 corpus cases |

## Enable

```dx
import .ts
rule es-no-invalid-void-type
```

## Examples

Incorrect:

```ts
type PossibleValues = string | number | void;
function logSomething(thing: void) {}
```

Correct:

```ts
type NoOp = () => void;
async function finish(): Promise<void> {}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `allowAsThisParameter` | `boolean` | `false` | Permit `this: void` parameters. |
| `allowInGenericTypeArguments` | `boolean` or `string[]` | `true` | Permit `void` in all generic arguments, none, or only named generic types. |

```dx
rule es-no-invalid-void-type {
  set allowInGenericTypeArguments ["Promise"]
}
```

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/no-invalid-void-type) documentation (MIT).
