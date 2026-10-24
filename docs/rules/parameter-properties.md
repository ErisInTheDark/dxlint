# parameter-properties

Controls TypeScript constructor parameter properties. By default, properties must be declared explicitly in the class instead of being created by constructor parameter modifiers.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/parameter-properties`](https://typescript-eslint.io/rules/parameter-properties) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 63 / 63 corpus cases |

## Enable

```dx
import .ts
rule parameter-properties
```

## Examples

Incorrect:

```ts
class Foo {
  constructor(public name: string) {}
}
```

Correct:

```ts
class Foo {
  public name: string;
  constructor(name: string) {
    this.name = name;
  }
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `prefer` | `"class-property" \| "parameter-property"` | `"class-property"` | Preferred declaration form. |

```dx
rule parameter-properties {
  set prefer "parameter-property"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `parameter-properties` | Constructor parameters that declare properties. |
| `parameter-properties:class` | Class properties that could become parameter properties. |

`rule parameter-properties` turns on every variant; `rule parameter-properties:class off` turns one off.

## Differences from upstream

- The upstream `allow` modifier list is not exposed.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/parameter-properties) documentation (MIT).
