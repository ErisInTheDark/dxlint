# no-restricted-properties

Reports configured object properties, whether read, called or destructured. Restrictions can target one object, one property across objects, or all properties on an object.

| | |
| --- | --- |
| Upstream | [`no-restricted-properties`](https://eslint.org/docs/latest/rules/no-restricted-properties) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 89 / 89 corpus cases |

## Enable

```dx
import .ts
rule no-restricted-properties
```

## Examples

With `options` set to `[{ object: "require", property: "resolve" }]`:

Incorrect:

```ts
require.resolve("foo");
const { resolve } = require;
```

Correct:

```ts
require("foo");
other.resolve("foo");
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | `restriction_property[]` | `[]` | Object/property restrictions and optional messages. |

Each record accepts `object`, `property` and `message`. Omit `object` to forbid the property on all objects; omit `property` to forbid all properties on that object. `allowObjects` exempts named objects from a property restriction; `allowProperties` exempts named properties from an object restriction.

```dx
rule no-restricted-properties {
  set options [{ object: "require", property: "resolve" }]
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `no-restricted-properties` | Dot and bracket property access. |
| `no-restricted-properties:patterns` | Destructured properties. |

`rule no-restricted-properties` turns on every variant; `rule no-restricted-properties:patterns off` turns one off.

## Differences from upstream

- Put ESLint's positional restriction records in the `options` array.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-restricted-properties) documentation (MIT).
