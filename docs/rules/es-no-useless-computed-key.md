# es-no-useless-computed-key

Reports literal property keys wrapped in unnecessary square brackets. Plain property syntax is shorter; computed keys whose removal would change the meaning are allowed.

| | |
| --- | --- |
| Upstream | [`no-useless-computed-key`](https://eslint.org/docs/latest/rules/no-useless-computed-key) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 96 / 96 corpus cases |

## Enable

```dx
import .ts
rule es-no-useless-computed-key
```

## Examples

Incorrect:

```ts
const object = { ["name"]: "value" };
class Example {
  ["read"]() {}
}
```

Correct:

```ts
const object = { name: "value" };
class Example {
  read() {}
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `enforceForClassMembers` | `boolean` | `true` | Also check class fields, methods, and accessors. |

```dx
rule es-no-useless-computed-key {
  set enforceForClassMembers false
}
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-useless-computed-key) documentation (MIT).
