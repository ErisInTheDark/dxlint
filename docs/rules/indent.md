# indent

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
rule indent
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

Offsets are measured in indentation levels. `indent_list_offset` means a nonnegative integer or `"first"` to align later items with the first item.

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `mode` | `int(0..) \| "tab"` | `4` | Spaces per level, or tabs. |
| `tab_length` | `number(1..)` | `4` | Raw-template tab width in tab mode. |
| `switch_case` | `int(0..)` | `1` | Case labels inside a switch. |
| `variable_declarator` | `indent_list_offset` | `1` | Continued variable declarations. |
| `variable_declarator_var` | `indent_list_offset \| null` | `null` | Override for var; null inherits. |
| `variable_declarator_let` | `indent_list_offset \| null` | `null` | Override for let; null inherits. |
| `variable_declarator_const` | `indent_list_offset \| null` | `null` | Override for const; null inherits. |
| `variable_declarator_using` | `indent_list_offset \| null` | `null` | Override for using; null inherits. |
| `member_expression` | `int(0..)` | `1` | Continued member access. |
| `function_declaration_parameters` | `indent_list_offset` | `1` | Declaration parameters. |
| `function_declaration_body` | `int(0..)` | `1` | Declaration bodies. |
| `function_declaration_return_type` | `int(0..)` | `1` | Declaration return types. |
| `function_expression_parameters` | `indent_list_offset` | `1` | Function expression and arrow parameters. |
| `function_expression_body` | `int(0..)` | `1` | Function expression and arrow bodies. |
| `function_expression_return_type` | `int(0..)` | `1` | Function expression return types. |
| `static_block` | `int(0..)` | `1` | Static block bodies. |
| `call_expression` | `indent_list_offset` | `1` | Call arguments. |
| `array_expression` | `indent_list_offset` | `1` | Array items. |
| `object_expression` | `indent_list_offset` | `1` | Object members. |
| `import_declaration` | `indent_list_offset` | `1` | Import specifiers. |
| `assignment_operator` | `int(0..)` | `1` | Continued assignments. |
| `outerIIFEBody` | `int(0..)` | `1` | Outer immediately invoked function body. |
| `flat_ternary_expressions` | `boolean` | `false` | Keep nested conditional branches flat. |
| `offset_ternary_expressions` | `boolean` | `false` | Add depth for grouped conditional branches. |

```dx
rule indent {
  set mode 2
  set switch_case 1
  set call_expression "first"
}
```

## Differences from upstream

- Most upstream object options become flat snake-case names, such as `FunctionDeclaration.parameters` becoming `function_declaration_parameters`.
- Upstream's `ignoredNodes`, `ignoreComments`, `"off"` offsets and object form of `offsetTernaryExpressions` are not exposed.
- Flow syntax is outside this TypeScript port's supported dialect.
- Babel `do` expressions inside JSX are not supported.
- Invalid arrow bodies containing only a parenthesised comment do not have upstream's parser recovery.
- Fixes for nested JSX text and blank lines can cover different ranges, even when diagnostic positions and counts agree.

Adapted from the [ESLint Stylistic](https://eslint.style/rules/indent) documentation (MIT).
