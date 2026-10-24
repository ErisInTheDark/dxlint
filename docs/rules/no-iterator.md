# no-iterator

Reports access to the obsolete SpiderMonkey `__iterator__` property. Use standard iterators and generators instead.

| | |
| --- | --- |
| Upstream | [`no-iterator`](https://eslint.org/docs/latest/rules/no-iterator) |
| Needs types | no |
| Fix | none |
| Rulesets | none |
| Parity | 9 / 9 corpus cases |

## Enable

```dx
import .ts
rule no-iterator
```

## Examples

Incorrect:

```ts
foo.__iterator__ = function () {};
```

Correct:

```ts
const __iterator__ = foo;
```

Adapted from the [ESLint](https://eslint.org/docs/latest/rules/no-iterator) documentation (MIT).
