# es-indent

Reports inconsistent leading indentation in JavaScript and TypeScript. Choose a space width or tabs, then adjust offsets for particular syntax where needed.

| | |
| --- | --- |
| Upstream | [`@stylistic/indent`](https://eslint.style/rules/indent) |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | 1306 / 1326 corpus cases |

## Enable

```dx
import .ts
rule es-indent
```

## Examples

Incorrect:

```ts
if (ready) {
  work();
}
```

Correct:

```ts
if (ready) {
    work();
}
```

## Options

`mode` is the default option: `rule es-indent set <value>` sets it.

Offsets are measured in indentation levels. `indent_list_offset` means a nonnegative integer or `"first"` to align later items with the first item.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `int(0..) \| "tab"` | `4` | Spaces per level, or tabs. |
| `tabLength` | `number(1..)` | `4` | Raw-template tab width in tab mode. |
| `switchCase` | `int(0..)` | `1` | Case labels inside a switch. |
| `variableDeclarator` | `indent_list_offset` | `1` | Continued variable declarations. |
| `variableDeclaratorVar` | `indent_list_offset \| null` | `null` | Override for var; null inherits. |
| `variableDeclaratorLet` | `indent_list_offset \| null` | `null` | Override for let; null inherits. |
| `variableDeclaratorConst` | `indent_list_offset \| null` | `null` | Override for const; null inherits. |
| `variableDeclaratorUsing` | `indent_list_offset \| null` | `null` | Override for using; null inherits. |
| `memberExpression` | `int(0..)` | `1` | Continued member access. |
| `functionDeclarationParameters` | `indent_list_offset` | `1` | Declaration parameters. |
| `functionDeclarationBody` | `int(0..)` | `1` | Declaration bodies. |
| `functionDeclarationReturnType` | `int(0..)` | `1` | Declaration return types. |
| `functionExpressionParameters` | `indent_list_offset` | `1` | Function expression and arrow parameters. |
| `functionExpressionBody` | `int(0..)` | `1` | Function expression and arrow bodies. |
| `functionExpressionReturnType` | `int(0..)` | `1` | Function expression return types. |
| `staticBlock` | `int(0..)` | `1` | Static block bodies. |
| `callExpression` | `indent_list_offset` | `1` | Call arguments. |
| `arrayExpression` | `indent_list_offset` | `1` | Array items. |
| `objectExpression` | `indent_list_offset` | `1` | Object members. |
| `importDeclaration` | `indent_list_offset` | `1` | Import specifiers. |
| `assignmentOperator` | `int(0..)` | `1` | Continued assignments. |
| `outerIIFEBody` | `int(0..)` | `1` | Outer immediately invoked function body. |
| `flatTernaryExpressions` | `boolean` | `false` | Keep nested conditional branches flat. |
| `offsetTernaryExpressions` | `boolean` | `false` | Add depth for grouped conditional branches. |

```dx
rule es-indent {
  set mode 2
  set switchCase 1
  set callExpression "first"
}
```

## Differences from upstream

- Most upstream object options become flat snake-case names, such as `FunctionDeclaration.parameters` becoming `functionDeclarationParameters`.
- Upstream's `ignoredNodes`, `ignoreComments`, `"off"` offsets and object form of `offsetTernaryExpressions` are not exposed.
- Flow syntax is outside this TypeScript port's supported dialect.
- Babel `do` expressions inside JSX are not supported.
- Invalid arrow bodies containing only a parenthesised comment do not have upstream's parser recovery.
- Fixes for nested JSX text and blank lines can cover different ranges, even when diagnostic positions and counts agree.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/indent) documentation (MIT).
