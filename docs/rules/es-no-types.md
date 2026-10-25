# es-no-types

Reports JSDoc tags with braced type annotations. Use TypeScript annotations for types and leave JSDoc for descriptions.

| | |
| --- | --- |
| Needs types | no |
| Fix | none |
| Rulesets | none |

## Enable

```dx
import .ts
rule es-no-types
```

## Examples

Incorrect:

```ts
/**
 * @param {string} name User name.
 */
function greet(name: string) {}
```

Correct:

```ts
/**
 * @param name User name.
 */
function greet(name: string) {}
```

The rule checks `@param`, `@arg`, `@argument`, `@return`, `@returns`, `@property`, `@prop`, `@throws` and `@yields` tags.
