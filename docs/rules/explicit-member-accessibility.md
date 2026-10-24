# explicit-member-accessibility

Require explicit visibility on class members, or forbid redundant `public` modifiers. Members are public by default, but an explicit policy makes access easier to see.

| | |
| --- | --- |
| Upstream | [`@typescript-eslint/explicit-member-accessibility`](https://typescript-eslint.io/rules/explicit-member-accessibility) |
| Needs types | no |
| Fix | autofix |
| Rulesets | `rules:conventions` |
| Parity | 63 / 63 corpus cases |

## Enable

```dx
import .ts
rule explicit-member-accessibility
```

## Examples

Incorrect:

```ts
class Animal {
  name: string;
  walk(): void {}
}
```

Correct:

```ts
class Animal {
  public name: string;
  public walk(): void {}
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `accessibility` | `"explicit"` / `"no-public"` / `"off"` | `"explicit"` | Require visibility, forbid `public`, or skip checking. |
| `ignoredMethodNames` | `string[]` | `[]` | Exempt named methods. |
| `overrides` | `{ properties?: string; methods?: string; accessors?: string; constructors?: string; parameterProperties?: string }` | `{ properties: null, methods: null, accessors: null, constructors: null, parameterProperties: null }` | Per-kind policy, using the same values as `accessibility`; null entries inherit it. |

```dx
rule explicit-member-accessibility {
  set accessibility "no-public"
}
```

## Variants

| Variant | Checks |
| --- | --- |
| `explicit-member-accessibility:property` | Class properties. |
| `explicit-member-accessibility:method` | Methods, constructors and accessors. |
| `explicit-member-accessibility:parameter` | Readonly constructor parameter properties. |

`rule explicit-member-accessibility` turns on every variant; `rule explicit-member-accessibility:parameter off` turns one off.

## Differences from upstream

- Autofixes remove forbidden `public` modifiers. Missing modifiers are reported without upstream's suggestions for choosing a visibility.

Adapted from the [typescript-eslint](https://typescript-eslint.io/rules/explicit-member-accessibility) documentation (MIT).
