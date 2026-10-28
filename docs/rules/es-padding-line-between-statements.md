# es-padding-line-between-statements

Requires or forbids blank lines between selected kinds of statements. With no policies configured, it reports nothing.

| | |
| --- | --- |
| Upstream | [`@stylistic/padding-line-between-statements`](https://eslint.style/rules/padding-line-between-statements) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 704 / 704 corpus cases |

## Enable

```dx
import .ts
rule es-padding-line-between-statements
```

## Examples

With `options` set to `[{ blankLine: "always", prev: "const", next: "return" }]`:

Incorrect:

```ts
function foo() {
  const a = 1;
  return a;
}
```

Correct:

```ts
function foo() {
  const a = 1;

  return a;
}
```

## Options

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `options` | Array of `{ blankLine, prev, next }` policies | `[]` | Ordered policies; the last matching policy wins. |
| `typescriptBodies` | boolean | `true` | Also pad statements in namespace bodies and members of interfaces and type literals, as `@stylistic` does. `false` checks only the statement lists ESLint core's `padding-line-between-statements` checks. |

`blankLine` is `"always"`, `"never"` or `"any"`. A comment-only line does not count as a blank line. `prev` and `next` accept a statement-kind string, a selector object or an array of either.

Statement kinds include `*`, `block`, `block-like`, `break`, `case`, `cjs-export`, `cjs-import`, `class`, `const`, `continue`, `debugger`, `default`, `directive`, `do`, `empty`, `export`, `expression`, `for`, `function`, `if`, `iife`, `import`, `let`, `return`, `switch`, `throw`, `try`, `using`, `var`, `while`, `with`, `enum`, `interface`, `type` and `function-overload`. A selector object has `selector: string` and optional `lineMode: "any" | "singleline" | "multiline"`.

```dx
rule es-padding-line-between-statements {
  set options [{
    blankLine: "always",
    prev: "const",
    next: "return"
  }]
}
```

## Differences from upstream

- Put upstream positional policies into the named `options` array.
- Porting ESLint core's `padding-line-between-statements`: set `typescriptBodies false`. Core never checks namespace bodies, so a stray `};` there is left alone.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/padding-line-between-statements) documentation (MIT).
