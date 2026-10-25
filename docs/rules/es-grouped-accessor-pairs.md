# es-grouped-accessor-pairs

Keep each property's getter and setter next to each other. An optional policy also fixes their order; this rule does not require both accessors to exist.

| | |
| --- | --- |
| Upstream | [`es-grouped-accessor-pairs`](https://eslint.org/docs/latest/rules/grouped-accessor-pairs) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 150 / 150 corpus cases |

## Enable

```dx
import .ts
rule es-grouped-accessor-pairs
```

## Examples

Incorrect:

```ts
const item = {
  get a() { return this.value; },
  b: 1,
  set a(value) { this.value = value; }
};
```

Correct:

```ts
const item = {
  get a() { return this.value; },
  set a(value) { this.value = value; },
  b: 1
};
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `"anyOrder"` / `"getBeforeSet"` / `"setBeforeGet"` | `"anyOrder"` | Required order within each pair. |
| `enforceForTSTypes` | `boolean` | `false` | Also check accessors in TypeScript type declarations. |

```dx
rule es-grouped-accessor-pairs {
  set mode "getBeforeSet"
  set enforceForTSTypes true
}
```

## Differences from upstream

- The positional order option is named `mode`.

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/grouped-accessor-pairs) documentation (MIT).
