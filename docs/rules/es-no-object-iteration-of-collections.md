# es-no-object-iteration-of-collections

Reports `Object.keys`, `Object.values`, `Object.entries` and `for-in` over a `Map`, `ReadonlyMap`, `WeakMap`, `Set`, `ReadonlySet` or `WeakSet` (subclasses included). Their entries are not own properties, so these always see nothing. Reported only when every type in a union is such a collection.

| | |
| --- | --- |
| Upstream | none |
| Needs types | yes |
| Fix | none |
| Rulesets | `rules:bugs` (error), `rules:recommended` |
| Parity | not measured |

## Enable

```dx
import .ts
rule es-no-object-iteration-of-collections
```

## Examples

Incorrect:

```ts
declare const chances: Map<string, number>;
for (const [name, chance] of Object.entries(chances)) {
  roll(name, chance);
}
declare const seen: Set<string>;
for (const key in seen) {}
```

Correct:

```ts
declare const chances: Map<string, number>;
for (const [name, chance] of chances) {
  roll(name, chance);
}
declare const seen: Set<string>;
for (const key of seen) {}
```
