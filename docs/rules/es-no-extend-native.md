# es-no-extend-native

Reports additions to built-in prototypes. These additions affect unrelated code and can conflict with future language features or other libraries.

| | |
| --- | --- |
| Upstream | [`es-no-extend-native`](https://eslint.org/docs/latest/rules/no-extend-native) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 40 / 40 corpus cases |

## Enable

```dx
import .ts
rule es-no-extend-native
```

## Examples

Incorrect:

```ts
Object.prototype.extra = 55;
Object.defineProperty(Array.prototype, "times", { value: 999 });
```

Correct:

```ts
const extra = 55;
function times() {
  return 999;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `exceptions` | `string[]` | `[]` | Names of built-ins whose prototypes may be extended. |

```dx
rule es-no-extend-native {
  set exceptions ["Object"]
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-extend-native) documentation (MIT).
