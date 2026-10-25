# Rules

Every rule dxlint ships, 433 in all. Turn one on with `rule <name>` after importing its language; see [basic.md](../basic.md).

- **Types**: the rule reads type information (from your `tsconfig.json`; TypeScript itself never runs).
- **Fix**: `autofix` rules are fixed by `dxlint --fix` and by the editor's quick fixes.
- **Parity**: the share of the upstream rule's own test cases where dxlint gives the same result. 337 of the 391 measured rules match every case; 43,352 of 43,622 cases match overall. Each page lists its known differences.

Pages are adapted from the documentation of [ESLint](https://eslint.org/docs/latest/rules/), [typescript-eslint](https://typescript-eslint.io/rules/), [ESLint Stylistic](https://eslint.style/rules) and [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import), all MIT licensed.

| Rule | What it reports | Types | Fix | Parity |
| --- | --- | --- | --- | --- |
| [es-accessor-pairs](es-accessor-pairs.md) | Reports properties with a setter but no getter. |  |  | 100% |
| [es-adjacent-overload-signatures](es-adjacent-overload-signatures.md) | Requires overload signatures for the same function or method to be next to each other. |  |  | 100% |
| [es-array-bracket-spacing](es-array-bracket-spacing.md) | Enforces spaces inside array brackets, including destructuring patterns. |  | autofix |  |
| [es-array-callback-return](es-array-callback-return.md) | Reports array callbacks that fail to return a value where one is needed, such as callbacks to `map`, `filter` and `reduce`. |  |  | 100% |
| [es-array-type](es-array-type.md) | Enforces one notation for TypeScript array types: `T[]` or `Array<T>`. |  | autofix | 100% |
| [es-arrow-body-style](es-arrow-body-style.md) | Controls whether arrow functions use block bodies or return expressions directly. |  | autofix | 100% |
| [es-arrow-parens](es-arrow-parens.md) | Enforces consistent parentheses around arrow parameters. |  | autofix | 93.6% |
| [es-arrow-spacing](es-arrow-spacing.md) | Controls spaces before and after an arrow function's `=>`. |  | autofix | 100% |
| [es-await-thenable](es-await-thenable.md) | Reports awaiting values that are not promises or other thenables. | yes |  | 97.7% |
| [es-ban-ts-comment](es-ban-ts-comment.md) | Controls TypeScript directive comments that suppress or enable compiler checks. |  |  | 100% |
| [es-ban-tslint-comment](es-ban-tslint-comment.md) | Reports obsolete TSLint enable and disable comments. |  | autofix | 100% |
| [es-block-scoped-var](es-block-scoped-var.md) | Reports `var` bindings used outside the block where they were declared. |  |  | 100% |
| [es-block-spacing](es-block-spacing.md) | Controls spaces immediately inside block braces when the next or previous token is on the same line. |  | autofix | 100% |
| [es-brace-style](es-brace-style.md) | Enforces consistent block brace placement. |  | autofix | 100% |
| [es-camelcase](es-camelcase.md) | Reports names with underscores between words instead of camel case. |  |  | 100% |
| [es-capitalized-comments](es-capitalized-comments.md) | Enforces the initial letter case of comments. |  | autofix | 96.6% |
| [es-check-alignment](es-check-alignment.md) | Reports misaligned JSDoc blocks. |  |  |  |
| [es-class-literal-property-style](es-class-literal-property-style.md) | Enforces one way to expose constant literal values on classes: readonly fields or getters. |  |  | 100% |
| [es-class-methods-use-this](es-class-methods-use-this.md) | Reports instance methods and function-valued fields that never use `this`. |  |  | 100% |
| [es-comma-dangle](es-comma-dangle.md) | Controls trailing commas in lists, including arrays, objects, calls, imports and TypeScript declarations. |  | autofix | 97.5% |
| [es-comma-spacing](es-comma-spacing.md) | Enforces spaces around commas. |  | autofix | 98.9% |
| [es-comma-style](es-comma-style.md) | Controls whether a comma belongs at the end of its item line or the start of the following line. |  | autofix | 100% |
| [es-complexity](es-complexity.md) | Limits cyclomatic complexity: the number of independent paths through a function. |  |  | 100% |
| [es-computed-property-spacing](es-computed-property-spacing.md) | Controls spaces inside computed property brackets, such as `object[key]` and `{ [key]: value }`. |  | autofix | 100% |
| [es-consistent-generic-constructors](es-consistent-generic-constructors.md) | Enforces one location for generic arguments when constructing a value: the type annotation or the constructor call. |  | autofix | 100% |
| [es-consistent-indexed-object-style](es-consistent-indexed-object-style.md) | Enforces one form for dictionary-like types: `Record` or an index signature. |  | autofix | 98.2% |
| [es-consistent-return](es-consistent-return.md) | Requires return statements in a function to consistently return a value or omit it. | yes |  | 100% |
| [es-consistent-this](es-consistent-this.md) | Require consistent names for variables that capture `this`. |  |  | 100% |
| [es-consistent-type-assertions](es-consistent-type-assertions.md) | Use one type assertion style throughout the project. |  | autofix | 100% |
| [es-consistent-type-definitions](es-consistent-type-definitions.md) | Use either interfaces or type aliases consistently for object shapes. |  | autofix | 100% |
| [es-consistent-type-exports](es-consistent-type-exports.md) | Mark exports that exist only in the type system with `type`. | yes | autofix | 100% |
| [es-consistent-type-imports](es-consistent-type-imports.md) | Use explicit type imports for names used only as types. |  | autofix | 100% |
| [es-constructor-super](es-constructor-super.md) | Require valid `super()` calls in constructors. |  |  | 100% |
| [es-curly](es-curly.md) | Require a consistent brace policy for conditional and loop bodies. |  | autofix | 100% |
| [es-default-case-last](es-default-case-last.md) | Put a switch's `default` clause after all its cases. |  |  | 100% |
| [es-default-case](es-default-case.md) | Require a `default` clause in switch statements, or a comment explaining its absence. |  |  | 100% |
| [es-default-param-last](es-default-param-last.md) | Put defaulted and optional parameters after required parameters. |  |  | 100% |
| [es-dot-location](es-dot-location.md) | Keep member-access dots on a consistent side of a line break. |  | autofix | 100% |
| [es-dot-notation](es-dot-notation.md) | Prefer dot notation when a property can be named directly. | yes | autofix | 100% |
| [es-eqeqeq](es-eqeqeq.md) | Prefer strict equality operators so comparisons do not silently coerce values. |  | autofix | 100% |
| [es-explicit-function-return-type](es-explicit-function-return-type.md) | Require return type annotations on functions and methods. |  |  | 100% |
| [es-explicit-member-accessibility](es-explicit-member-accessibility.md) | Require explicit visibility on class members, or forbid redundant `public` modifiers. |  | autofix | 100% |
| [es-explicit-module-boundary-types](es-explicit-module-boundary-types.md) | Require argument and return types on exported functions and public methods of exported classes. |  |  | 100% |
| [es-for-direction](es-for-direction.md) | Report for-loop counters that move away from their stop condition. |  |  | 100% |
| [es-func-name-matching](es-func-name-matching.md) | Match a named function expression's name to its variable or property name. |  |  | 100% |
| [es-func-names](es-func-names.md) | Require or forbid names on function expressions. |  |  | 100% |
| [es-func-style](es-func-style.md) | Choose function declarations or expressions for functions assigned to names. |  |  | 100% |
| [es-function-call-spacing](es-function-call-spacing.md) | Require or forbid whitespace before invocation parentheses. |  | autofix | 100% |
| [es-generator-star-spacing](es-generator-star-spacing.md) | Keep spacing around generator stars consistent. |  | autofix | 100% |
| [es-getter-return](es-getter-return.md) | Require getters to return a value on paths that finish normally. |  |  | 100% |
| [es-grouped-accessor-pairs](es-grouped-accessor-pairs.md) | Keep each property's getter and setter next to each other. |  |  | 100% |
| [es-guard-for-in](es-guard-for-in.md) | Require a guard inside for-in loops to filter unwanted properties. |  |  | 100% |
| [es-header](es-header.md) | Require a configured licence comment at the start of a file. |  | autofix |  |
| [es-id-denylist](es-id-denylist.md) | Reports identifiers on a list of forbidden names. |  |  | 100% |
| [es-id-length](es-id-length.md) | Reports identifier names shorter or longer than the configured limits. |  |  | 100% |
| [es-id-match](es-id-match.md) | Reports identifier names that do not match a regular expression. |  |  | 100% |
| [es-implicit-arrow-linebreak](es-implicit-arrow-linebreak.md) | Requires a consistent position for an arrow function's implicitly returned expression. |  | autofix | 100% |
| [es-import-no-deprecated](es-import-no-deprecated.md) | Reports imports and uses of exports marked with JSDoc `@deprecated`. | yes |  |  |
| [es-indent-binary-ops](es-indent-binary-ops.md) | Reports uneven indentation in multiline binary expressions. |  | autofix | 100% |
| [es-indent](es-indent.md) | Reports inconsistent leading indentation in JavaScript and TypeScript. |  | autofix | 98.5% |
| [es-init-declarations](es-init-declarations.md) | Requires variables to be initialised when declared, or forbids initialisers with `"never"`. |  |  | 100% |
| [es-jsx-child-element-spacing](es-jsx-child-element-spacing.md) | Reports ambiguous spacing between JSX text and inline elements on neighbouring lines. |  |  | 100% |
| [es-jsx-closing-bracket-location](es-jsx-closing-bracket-location.md) | Checks where the final `>` or `/>` of a multiline JSX opening tag appears. |  | autofix | 100% |
| [es-jsx-closing-tag-location](es-jsx-closing-tag-location.md) | Requires the closing tag of multiline JSX to be on its own line and correctly aligned. |  | autofix | 100% |
| [es-jsx-curly-brace-presence](es-jsx-curly-brace-presence.md) | Reports unnecessary braces around JSX string literals, or requires braces when configured. |  | autofix | 99.3% |
| [es-jsx-curly-newline](es-jsx-curly-newline.md) | Checks line breaks directly inside JSX expression braces. |  | autofix | 100% |
| [es-jsx-curly-spacing](es-jsx-curly-spacing.md) | Requires or forbids spaces inside JSX expression braces. |  | autofix | 100% |
| [es-jsx-equals-spacing](es-jsx-equals-spacing.md) | Requires consistent spaces around `=` in JSX attributes. |  | autofix | 100% |
| [es-jsx-first-prop-new-line](es-jsx-first-prop-new-line.md) | Checks whether the first JSX prop starts beside the tag name or on a new line. |  | autofix | 100% |
| [es-jsx-function-call-newline](es-jsx-function-call-newline.md) | Requires line breaks around JSX used as a function argument. |  | autofix | 100% |
| [es-jsx-indent-props](es-jsx-indent-props.md) | Reports inconsistent indentation of JSX props that start a new line. |  | autofix | 100% |
| [es-jsx-max-props-per-line](es-jsx-max-props-per-line.md) | Limits how many JSX props can share a line. |  | autofix | 100% |
| [es-jsx-newline](es-jsx-newline.md) | Requires or prevents blank lines between adjacent JSX elements and expressions. |  | autofix | 100% |
| [es-jsx-one-expression-per-line](es-jsx-one-expression-per-line.md) | Requires JSX children to occupy separate lines rather than sharing a line with other children or their parent tags. |  | autofix | 100% |
| [es-jsx-pascal-case](es-jsx-pascal-case.md) | Reports user-defined JSX component names that are not PascalCase. |  |  | 100% |
| [es-jsx-props-style](es-jsx-props-style.md) | Keeps JSX prop wrapping consistent with the first prop's position. |  | autofix | 100% |
| [es-jsx-quotes](es-jsx-quotes.md) | Requires consistent quote marks around JSX attribute strings. |  | autofix | 100% |
| [es-jsx-self-closing-comp](es-jsx-self-closing-comp.md) | Reports JSX elements with an unnecessary separate closing tag because they have no children. |  | autofix | 100% |
| [es-jsx-shorthand-boolean](es-jsx-shorthand-boolean.md) | Reports JSX attributes explicitly set to `true`. |  | autofix | 100% |
| [es-jsx-shorthand-fragment](es-jsx-shorthand-fragment.md) | Prefers `<>...</>` over `Fragment` and `React.Fragment` when no props are needed. |  | autofix | 100% |
| [es-jsx-tag-spacing](es-jsx-tag-spacing.md) | Checks spaces beside JSX angle brackets and slashes. |  | autofix | 100% |
| [es-jsx-wrap-multilines](es-jsx-wrap-multilines.md) | Reports multiline JSX without the configured parentheses. |  | autofix | 100% |
| [es-key-spacing](es-key-spacing.md) | Reports inconsistent spaces around property colons. |  | autofix | 100% |
| [es-keyword-spacing](es-keyword-spacing.md) | Reports missing or unwanted spaces around keywords. |  | autofix | 100% |
| [es-line-comment-position](es-line-comment-position.md) | Reports line comments in the wrong position: beside code or on their own line. |  |  | 86.1% |
| [es-lines-around-comment](es-lines-around-comment.md) | Reports missing blank lines around standalone comments. |  | autofix | 100% |
| [es-lines-between-class-members](es-lines-between-class-members.md) | Reports missing or unwanted blank lines between class members. |  | autofix | 100% |
| [es-list-style](es-list-style.md) | Reports inconsistent spacing and wrapping inside brackets. |  | autofix | 100% |
| [es-logical-assignment-operators](es-logical-assignment-operators.md) | Reports assignments that can use `\|\|=`, `&&=`, or `??=`. |  | autofix | 100% |
| [es-max-classes-per-file](es-max-classes-per-file.md) | Reports files containing more classes than the configured limit. |  |  | 94.7% |
| [es-max-depth](es-max-depth.md) | Reports control-flow blocks nested beyond the limit. |  |  | 100% |
| [es-max-len](es-max-len.md) | Reports lines longer than the configured limit. |  |  | 100% |
| [es-max-lines-per-function](es-max-lines-per-function.md) | Reports functions with too many lines. |  |  | 100% |
| [es-max-lines](es-max-lines.md) | Reports files with too many lines. |  |  | 100% |
| [es-max-nested-callbacks](es-max-nested-callbacks.md) | Reports callbacks nested beyond the configured limit. |  |  | 100% |
| [es-max-params](es-max-params.md) | Reports functions with too many parameters. |  |  | 100% |
| [es-max-statements-per-line](es-max-statements-per-line.md) | Reports lines containing too many statements. |  |  | 100% |
| [es-max-statements](es-max-statements.md) | Reports functions containing too many statements. |  |  | 100% |
| [es-member-delimiter-style](es-member-delimiter-style.md) | Reports inconsistent separators between interface and type literal members. |  | autofix | 100% |
| [es-member-ordering](es-member-ordering.md) | Reports members declared out of order in classes, interfaces, and type literals. |  |  | 100% |
| [es-method-signature-style](es-method-signature-style.md) | Reports function members written in the wrong signature style. |  | autofix | 100% |
| [es-multiline-comment-style](es-multiline-comment-style.md) | Reports multiline comments that do not follow the selected shape. |  | autofix | 95.5% |
| [es-multiline-ternary](es-multiline-ternary.md) | Reports missing or unwanted line breaks between conditional expression operands. |  | autofix | 100% |
| [es-naming-convention](es-naming-convention.md) | Reports names that do not follow the convention for their declaration kind. |  |  | 97.8% |
| [es-new-cap](es-new-cap.md) | Reports lowercase constructor names and capitalised functions called without `new`. |  |  | 100% |
| [es-new-parens](es-new-parens.md) | Reports missing or unwanted empty parentheses in constructor calls. |  | autofix | 100% |
| [es-newline-per-chained-call](es-newline-per-chained-call.md) | Reports chained calls that need a preceding line break. |  | autofix | 100% |
| [es-no-alert](es-no-alert.md) | Reports calls to the global `alert`, `confirm` and `prompt` functions. |  |  | 100% |
| [es-no-array-constructor](es-no-array-constructor.md) | Reports array constructor calls that can use array literals instead. |  | autofix | 100% |
| [es-no-array-delete](es-no-array-delete.md) | Reports deleting an array element. | yes |  | 100% |
| [es-no-async-promise-executor](es-no-async-promise-executor.md) | Reports async functions passed as `Promise` executors. |  |  | 100% |
| [es-no-await-in-loop](es-no-await-in-loop.md) | Reports `await` operations inside loops, including `await using`. |  |  | 100% |
| [es-no-base-to-string](es-no-base-to-string.md) | Reports string conversions that use an object's unhelpful default representation, such as `"[object Object]"`. | yes |  | 98.4% |
| [es-no-bitwise](es-no-bitwise.md) | Reports bitwise operators. |  |  | 100% |
| [es-no-blank-lines-in-optional-blocks](es-no-blank-lines-in-optional-blocks.md) | Reports blank lines between an `if` or loop header and its single unbraced statement. |  | autofix |  |
| [es-no-caller](es-no-caller.md) | Reports `arguments.caller` and `arguments.callee`. |  |  | 100% |
| [es-no-case-declarations](es-no-case-declarations.md) | Reports lexical declarations directly inside `case` or `default` clauses. |  |  | 100% |
| [es-no-class-assign](es-no-class-assign.md) | Reports assignments to class declaration bindings. |  |  | 100% |
| [es-no-compare-neg-zero](es-no-compare-neg-zero.md) | Reports comparisons against `-0`. |  |  | 100% |
| [es-no-cond-assign](es-no-cond-assign.md) | Reports assignments used as conditions where a comparison may have been intended. |  |  | 100% |
| [es-no-confusing-arrow](es-no-confusing-arrow.md) | Reports arrow functions whose conditional expression bodies can look like comparisons. |  | autofix | 100% |
| [es-no-confusing-non-null-assertion](es-no-confusing-non-null-assertion.md) | Reports non-null assertions next to assignment, equality, `in` or `instanceof` operators. |  |  | 100% |
| [es-no-confusing-void-expression](es-no-confusing-void-expression.md) | Reports `void`-typed expressions used where a value appears to be needed. | yes | autofix | 99.1% |
| [es-no-console](es-no-console.md) | Reports use of `console` methods, including assignments to them. |  |  | 100% |
| [es-no-const-assign](es-no-const-assign.md) | Reports writes to constant bindings. |  |  | 100% |
| [es-no-constant-binary-expression](es-no-constant-binary-expression.md) | Reports comparisons whose result is fixed and logical expressions that always or never short-circuit. |  |  | 100% |
| [es-no-constant-condition](es-no-constant-condition.md) | Reports conditions whose truthiness cannot change. |  |  | 100% |
| [es-no-constructor-return](es-no-constructor-return.md) | Reports return statements with values inside class constructors. |  |  | 100% |
| [es-no-continue](es-no-continue.md) | Reports `continue` statements, including labelled ones. |  |  | 100% |
| [es-no-control-regex](es-no-control-regex.md) | Reports raw ASCII control characters and hexadecimal or Unicode escapes for them in regular expressions. |  |  | 100% |
| [es-no-debugger](es-no-debugger.md) | Reports `debugger` statements. |  |  | 100% |
| [es-no-delete-var](es-no-delete-var.md) | Reports use of `delete` on a variable. |  |  | 100% |
| [es-no-deprecated](es-no-deprecated.md) | Reports references to symbols marked with a JSDoc `@deprecated` tag. | yes |  | 92.5% |
| [es-no-div-regex](es-no-div-regex.md) | Reports regular expression literals starting with `=`. |  | autofix | 100% |
| [es-no-dupe-args](es-no-dupe-args.md) | Reports repeated parameter names in function declarations and expressions. |  |  | 100% |
| [es-no-dupe-class-members](es-no-dupe-class-members.md) | Reports class members with the same name when one would overwrite another. |  |  | 100% |
| [es-no-dupe-else-if](es-no-dupe-else-if.md) | Reports conditions already covered by earlier branches in an `if` / `else if` chain. |  |  | 100% |
| [es-no-dupe-keys](es-no-dupe-keys.md) | Reports repeated keys in object literals. |  |  | 100% |
| [es-no-duplicate-case](es-no-duplicate-case.md) | Reports repeated test expressions in a `switch` statement. |  |  | 100% |
| [es-no-duplicate-enum-values](es-no-duplicate-enum-values.md) | Reports enum members explicitly assigned the same literal value. |  |  | 100% |
| [es-no-duplicate-imports](es-no-duplicate-imports.md) | Reports imports and re-exports from the same module when their declarations can be combined. |  |  | 100% |
| [es-no-duplicate-type-constituents](es-no-duplicate-type-constituents.md) | Reports repeated types in unions and intersections, including aliases that resolve to the same type. | yes | autofix | 100% |
| [es-no-dynamic-delete](es-no-dynamic-delete.md) | Reports `delete` with a computed key that is not a string or number literal. |  |  | 100% |
| [es-no-else-return](es-no-else-return.md) | Reports an `else` after a branch that always returns. |  | autofix | 100% |
| [es-no-empty-character-class](es-no-empty-character-class.md) | Reports empty character classes in regular expression literals. |  |  | 100% |
| [es-no-empty-function](es-no-empty-function.md) | Reports functions with empty bodies and no explanatory comment. |  |  | 100% |
| [es-no-empty-interface](es-no-empty-interface.md) | Reports interfaces with no members and at most one supertype. |  |  |  |
| [es-no-empty-object-type](es-no-empty-object-type.md) | Reports empty interfaces and `{}` type literals. |  |  | 100% |
| [es-no-empty-pattern](es-no-empty-pattern.md) | Reports empty destructuring patterns. |  |  | 100% |
| [es-no-empty-static-block](es-no-empty-static-block.md) | Reports empty class static blocks. |  |  | 100% |
| [es-no-empty](es-no-empty.md) | Reports empty blocks and empty `switch` statements. |  |  | 100% |
| [es-no-eq-null](es-no-eq-null.md) | Reports loose equality comparisons with `null`. |  |  | 100% |
| [es-no-eval](es-no-eval.md) | Reports calls and references to `eval`. |  |  | 100% |
| [es-no-ex-assign](es-no-ex-assign.md) | Reports assignments to a caught exception parameter. |  |  | 100% |
| [es-no-explicit-any](es-no-explicit-any.md) | Reports explicit `any` types, which bypass type checking. |  | autofix (with `fixToUnknown`) | 100% |
| [es-no-extend-native](es-no-extend-native.md) | Reports additions to built-in prototypes. |  |  | 100% |
| [es-no-extra-bind](es-no-extra-bind.md) | Reports `.bind()` calls that do not change the function's behaviour. |  | autofix | 100% |
| [es-no-extra-boolean-cast](es-no-extra-boolean-cast.md) | Reports boolean conversions in places that already convert their value to a boolean. |  | autofix | 100% |
| [es-no-extra-label](es-no-extra-label.md) | Reports labels on `break` and `continue` when the same statement would be targeted without the label. |  | autofix | 100% |
| [es-no-extra-non-null-assertion](es-no-extra-non-null-assertion.md) | Reports non-null assertions that add nothing, such as repeated `!` assertions. |  | autofix | 100% |
| [es-no-extra-parens](es-no-extra-parens.md) | Reports parentheses that do not affect an expression's meaning. |  | autofix | 100% |
| [es-no-extra-semi](es-no-extra-semi.md) | Reports unnecessary semicolons, such as repeated statement terminators or semicolons after function declarations. |  | autofix | 100% |
| [es-no-extraneous-class](es-no-extraneous-class.md) | Reports empty classes, constructor-only classes and classes used only as static namespaces. |  |  | 100% |
| [es-no-fallthrough](es-no-fallthrough.md) | Reports switch cases that run into the next case without a break or other exit. |  |  | 96.6% |
| [es-no-floating-decimal](es-no-floating-decimal.md) | Requires digits on both sides of a decimal point. |  | autofix | 100% |
| [es-no-floating-promises](es-no-floating-promises.md) | Reports promise expressions whose rejection is not handled. | yes |  | 92.3% |
| [es-no-for-in-array](es-no-for-in-array.md) | Reports `for-in` loops over arrays. | yes |  | 100% |
| [es-no-func-assign](es-no-func-assign.md) | Reports writes to function declaration names. |  |  | 100% |
| [es-no-generated-empty-object-type](es-no-generated-empty-object-type.md) | Reports type operations that resolve to the empty object type, `{}`. | yes |  | 92.6% |
| [es-no-global-assign](es-no-global-assign.md) | Reports assignments to read-only globals. |  |  | 100% |
| [es-no-implicit-coercion](es-no-implicit-coercion.md) | Reports shorthand type conversions such as `!!value`, `+value`, and `"" + value`. |  | autofix | 100% |
| [es-no-implicit-globals](es-no-implicit-globals.md) | Reports declarations and assignments that pollute script global scope. |  |  | 100% |
| [es-no-implied-eval](es-no-implied-eval.md) | Reports code evaluated from strings by timers, `execScript`, or the `Function` constructor. | yes |  | 100% |
| [es-no-import-assign](es-no-import-assign.md) | Reports writes to imported bindings and namespace import members. |  |  | 100% |
| [es-no-import-type-side-effects](es-no-import-type-side-effects.md) | Requires a top-level `import type` when every named specifier is type-only. |  | autofix | 100% |
| [es-no-inferrable-types](es-no-inferrable-types.md) | Reports type annotations already clear from an initial or default value. |  | autofix | 100% |
| [es-no-inline-comments](es-no-inline-comments.md) | Reports comments on the same line as code. |  |  | 100% |
| [es-no-inner-declarations](es-no-inner-declarations.md) | Reports function declarations, and optionally `var` declarations, nested below a program or function body. |  |  | 100% |
| [es-no-invalid-regexp](es-no-invalid-regexp.md) | Reports invalid patterns and flags passed to `RegExp`. |  |  | 100% |
| [es-no-invalid-this](es-no-invalid-this.md) | Reports `this` where it may be undefined in strict code. |  |  | 100% |
| [es-no-invalid-void-type](es-no-invalid-void-type.md) | Reports `void` types outside supported return and generic type positions. |  |  | 100% |
| [es-no-irregular-whitespace](es-no-irregular-whitespace.md) | Reports unusual whitespace characters that are easy to overlook or confuse with ordinary spaces. |  |  | 100% |
| [es-no-iterator](es-no-iterator.md) | Reports access to the obsolete SpiderMonkey `__iterator__` property. |  |  | 100% |
| [es-no-label-var](es-no-label-var.md) | Reports statement labels sharing a name with an in-scope variable. |  |  | 100% |
| [es-no-labels](es-no-labels.md) | Reports labeled statements and labeled `break` or `continue`. |  |  | 100% |
| [es-no-lone-blocks](es-no-lone-blocks.md) | Reports standalone blocks that provide no useful scope. |  |  | 100% |
| [es-no-lonely-if](es-no-lonely-if.md) | Reports an `if` that is the only statement in an `else` block. |  | autofix | 100% |
| [es-no-loop-func](es-no-loop-func.md) | Reports functions created in loops that capture variables modified outside their own scope. |  |  | 100% |
| [es-no-loss-of-precision](es-no-loss-of-precision.md) | Reports number literals whose written precision is lost when represented as a JavaScript number. |  |  | 100% |
| [es-no-magic-numbers](es-no-magic-numbers.md) | Reports numeric literals used without a meaningful name. |  |  | 100% |
| [es-no-meaningless-void-operator](es-no-meaningless-void-operator.md) | Reports `void` when it does not discard a useful call result. | yes | autofix | 95.3% |
| [es-no-misleading-character-class](es-no-misleading-character-class.md) | Reports character classes that split a visible Unicode character into separate parts. |  |  | 100% |
| [es-no-misused-new](es-no-misused-new.md) | Reports constructor-like declarations in the wrong place: a class method named `new`, an interface constructing itself, or an interface/type member named `constructor`. |  |  | 100% |
| [es-no-misused-promises](es-no-misused-promises.md) | Reports promises used where a boolean, ordinary object, or synchronous callback is expected. | yes |  | 96.4% |
| [es-no-misused-spread](es-no-misused-spread.md) | Reports spreads whose types suggest a likely mistake, such as promises, functions, maps, arrays, or class instances copied into objects. | yes |  | 99.3% |
| [es-no-mixed-enums](es-no-mixed-enums.md) | Reports enums containing both numeric and string members. | yes |  | 100% |
| [es-no-mixed-operators](es-no-mixed-operators.md) | Requires parentheses around mixtures of operators in the same configured group. |  |  | 100% |
| [es-no-mixed-spaces-and-tabs](es-no-mixed-spaces-and-tabs.md) | Reports indentation containing both tabs and spaces. |  |  | 100% |
| [es-no-multi-assign](es-no-multi-assign.md) | Reports chained assignments. |  |  | 100% |
| [es-no-multi-spaces](es-no-multi-spaces.md) | Reports repeated spaces between code tokens, rather than indentation or spaces inside strings and comments. |  | autofix | 100% |
| [es-no-multi-str](es-no-multi-str.md) | Reports quoted strings continued across physical lines with a backslash. |  |  | 100% |
| [es-no-multiple-empty-lines](es-no-multiple-empty-lines.md) | Limits runs of blank lines to reduce unnecessary scrolling. |  | autofix | 100% |
| [es-no-namespace](es-no-namespace.md) | Reports TypeScript namespaces and old-style named modules in ordinary source files. |  |  | 100% |
| [es-no-negated-condition](es-no-negated-condition.md) | Reports negated conditions in ternaries and `if` statements with an `else` branch. |  |  | 100% |
| [es-no-nested-ternary](es-no-nested-ternary.md) | Reports ternary expressions nested inside another ternary's result branches. |  |  | 100% |
| [es-no-new-func](es-no-new-func.md) | Reports use of the global `Function` constructor, with or without `new`. |  |  | 100% |
| [es-no-new-native-nonconstructor](es-no-new-native-nonconstructor.md) | Reports `new Symbol` and `new BigInt`. |  |  | 100% |
| [es-no-new-wrappers](es-no-new-wrappers.md) | Reports `new String`, `new Number`, and `new Boolean`. |  |  | 100% |
| [es-no-new](es-no-new.md) | Reports constructor calls used as standalone statements, discarding the new object. |  |  | 100% |
| [es-no-non-null-asserted-nullish-coalescing](es-no-non-null-asserted-nullish-coalescing.md) | Reports non-null assertions on the left side of `??`. |  |  | 100% |
| [es-no-non-null-asserted-optional-chain](es-no-non-null-asserted-optional-chain.md) | Reports non-null assertions applied to optional-chain results. |  |  | 100% |
| [es-no-non-null-assertion](es-no-non-null-assertion.md) | Reports TypeScript's postfix `!` non-null assertion. |  |  | 100% |
| [es-no-nonoctal-decimal-escape](es-no-nonoctal-decimal-escape.md) | Reports `\8` and `\9` in string literals. |  |  | 100% |
| [es-no-obj-calls](es-no-obj-calls.md) | Reports calls and constructor calls to built-in objects that are not functions, such as `Math` and `JSON`. |  |  | 100% |
| [es-no-object-constructor](es-no-object-constructor.md) | Reports calls to `Object` without an argument, with or without `new`. |  |  | 100% |
| [es-no-octal-escape](es-no-octal-escape.md) | Reports legacy octal escape sequences in strings. |  |  | 100% |
| [es-no-octal](es-no-octal.md) | Reports legacy octal number literals such as `071`. |  |  | 100% |
| [es-no-param-reassign](es-no-param-reassign.md) | Reports writes to function parameters. |  |  | 100% |
| [es-no-plusplus](es-no-plusplus.md) | Reports the increment and decrement operators, `++` and `--`. |  |  | 100% |
| [es-no-promise-executor-return](es-no-promise-executor-return.md) | Reports returned values in Promise executor functions. |  |  | 100% |
| [es-no-proto](es-no-proto.md) | Reports access to the legacy `__proto__` property. |  |  | 100% |
| [es-no-prototype-builtins](es-no-prototype-builtins.md) | Reports direct calls to `hasOwnProperty`, `isPrototypeOf` and `propertyIsEnumerable` on objects. |  |  | 100% |
| [es-no-redeclare](es-no-redeclare.md) | Reports names declared more than once in the same scope. |  |  | 99.2% |
| [es-no-redundant-type-constituents](es-no-redundant-type-constituents.md) | Reports union and intersection members that add no useful type information. | yes |  | 100% |
| [es-no-regex-spaces](es-no-regex-spaces.md) | Reports consecutive literal spaces in regular expressions. |  | autofix | 100% |
| [es-no-require-imports](es-no-require-imports.md) | Reports CommonJS `require()` imports in favour of ES module syntax. |  |  | 100% |
| [es-no-restricted-exports](es-no-restricted-exports.md) | Reports exported names listed in your project's restrictions. |  |  | 100% |
| [es-no-restricted-globals](es-no-restricted-globals.md) | Reports uses of configured global names. |  |  | 100% |
| [es-no-restricted-imports](es-no-restricted-imports.md) | Reports imports and re-exports forbidden by module path or imported name. |  |  | 99.6% |
| [es-no-restricted-properties](es-no-restricted-properties.md) | Reports configured object properties, whether read, called or destructured. |  |  | 100% |
| [es-no-restricted-syntax](es-no-restricted-syntax.md) | Reports syntax added to the open `restricted_syntax` group. |  |  | 97.1% |
| [es-no-restricted-types](es-no-restricted-types.md) | Reports types forbidden by your configuration. |  | autofix | 100% |
| [es-no-return-assign](es-no-return-assign.md) | Reports assignments inside returned expressions. |  |  | 100% |
| [es-no-script-url](es-no-script-url.md) | Reports `javascript:` URLs in strings and template literals. |  |  | 100% |
| [es-no-self-assign](es-no-self-assign.md) | Reports assignments that copy a value back onto itself. |  |  | 100% |
| [es-no-self-compare](es-no-self-compare.md) | Reports comparisons with the same expression on both sides. |  |  | 100% |
| [es-no-sequences](es-no-sequences.md) | Reports comma operators that hide multiple operations in one expression. |  |  | 100% |
| [es-no-setter-return](es-no-setter-return.md) | Reports values returned from setters, where the caller cannot use them. |  |  | 100% |
| [es-no-shadow-restricted-names](es-no-shadow-restricted-names.md) | Reports bindings that replace restricted names such as `NaN`, `Infinity`, `undefined`, `eval` and `arguments`. |  |  | 100% |
| [es-no-shadow](es-no-shadow.md) | Reports declarations that hide names from an outer scope. |  |  | 100% |
| [es-no-sparse-arrays](es-no-sparse-arrays.md) | Reports holes in array literals, often caused by an accidental extra comma. |  |  | 100% |
| [es-no-sync](es-no-sync.md) | Reports calls to `Sync`-suffixed methods inside functions. |  |  |  |
| [es-no-tabs](es-no-tabs.md) | Reports tab characters anywhere in a file, including comments and strings. |  |  | 100% |
| [es-no-template-curly-in-string](es-no-template-curly-in-string.md) | Reports `${...}` placeholders inside ordinary quoted strings. |  |  | 100% |
| [es-no-ternary](es-no-ternary.md) | Reports conditional expressions using `? |  |  | 100% |
| [es-no-this-alias](es-no-this-alias.md) | Reports variables used to alias `this`. |  |  | 100% |
| [es-no-this-before-super](es-no-this-before-super.md) | Reports use of `this` or a `super` property before `super()` in a derived constructor. |  |  | 100% |
| [es-no-throw-literal](es-no-throw-literal.md) | Reports thrown expressions that cannot be error objects. |  |  | 100% |
| [es-no-types](es-no-types.md) | Reports JSDoc tags with braced type annotations. |  |  |  |
| [es-no-unassigned-import](es-no-unassigned-import.md) | Reports imports without imported bindings. |  |  |  |
| [es-no-unassigned-vars](es-no-unassigned-vars.md) | Reports variables that are read but never assigned a value. |  |  | 100% |
| [es-no-undef-init](es-no-undef-init.md) | Reports `let` and `var` declarations explicitly initialised to `undefined`. |  | autofix | 100% |
| [es-no-undef](es-no-undef.md) | Reports references to names that have not been declared. |  |  | 100% |
| [es-no-undefined](es-no-undefined.md) | Reports use of the identifier `undefined`. |  |  | 100% |
| [es-no-underscore-dangle](es-no-underscore-dangle.md) | Reports names with leading or trailing underscores. |  |  | 100% |
| [es-no-unexpected-multiline](es-no-unexpected-multiline.md) | Reports confusing line breaks that continue an expression instead of starting a new statement. |  |  | 100% |
| [es-no-unmodified-loop-condition](es-no-unmodified-loop-condition.md) | Reports variables used in a loop condition but never modified in the loop. |  |  | 100% |
| [es-no-unnecessary-boolean-literal-compare](es-no-unnecessary-boolean-literal-compare.md) | Reports equality comparisons between booleans and `true` or `false`. | yes | autofix | 97.2% |
| [es-no-unnecessary-condition](es-no-unnecessary-condition.md) | Reports conditions whose types make the result fixed. | yes |  | 96.9% |
| [es-no-unnecessary-parameter-property-assignment](es-no-unnecessary-parameter-property-assignment.md) | Reports assignments that repeat a constructor parameter property's automatic assignment. |  |  | 100% |
| [es-no-unnecessary-qualifier](es-no-unnecessary-qualifier.md) | Reports enum and namespace qualifiers when the member is already in scope. | yes | autofix | 100% |
| [es-no-unnecessary-template-expression](es-no-unnecessary-template-expression.md) | Reports template substitutions that can be incorporated into the surrounding text, or replaced with the string expression itself. | yes | autofix | 100% |
| [es-no-unnecessary-type-arguments](es-no-unnecessary-type-arguments.md) | Reports explicit type arguments equal to the declared default. | yes | autofix | 100% |
| [es-no-unnecessary-type-assertion](es-no-unnecessary-type-assertion.md) | Reports assertions that do not change an expression's type. | yes | autofix | 93.9% |
| [es-no-unnecessary-type-constraint](es-no-unnecessary-type-constraint.md) | Reports generic type parameters constrained to `any` or `unknown`. |  |  | 100% |
| [es-no-unnecessary-type-conversion](es-no-unnecessary-type-conversion.md) | Reports conversion idioms that do not change a value's type or value. | yes |  | 100% |
| [es-no-unnecessary-type-parameters](es-no-unnecessary-type-parameters.md) | Reports type parameters that do not relate multiple parts of a signature. | yes |  | 100% |
| [es-no-unneeded-ternary](es-no-unneeded-ternary.md) | Reports ternaries that can be replaced with simpler boolean expressions. |  | autofix | 100% |
| [es-no-unreachable-loop](es-no-unreachable-loop.md) | Reports loops whose bodies always exit before a second iteration. |  |  | 100% |
| [es-no-unreachable](es-no-unreachable.md) | Reports code that cannot run after an unconditional exit. |  |  | 100% |
| [es-no-unsafe-argument](es-no-unsafe-argument.md) | Reports `any` values passed to typed parameters. | yes |  | 80.0% |
| [es-no-unsafe-assignment](es-no-unsafe-assignment.md) | Reports assignments of `any` to variables and properties. | yes |  | 100% |
| [es-no-unsafe-call](es-no-unsafe-call.md) | Reports calls through values typed as `any` or the unsafe `Function` type. | yes |  | 97.4% |
| [es-no-unsafe-declaration-merging](es-no-unsafe-declaration-merging.md) | Reports merging a class and interface with the same name. |  |  | 100% |
| [es-no-unsafe-enum-assignment](es-no-unsafe-enum-assignment.md) | Reports values that do not share the enum type expected by an assignment, argument, return, or other receiving location. | yes |  | 88.0% |
| [es-no-unsafe-enum-comparison](es-no-unsafe-enum-comparison.md) | Reports comparisons between enum values and values without a shared enum type, including switch cases. | yes |  | 100% |
| [es-no-unsafe-finally](es-no-unsafe-finally.md) | Reports `return`, `throw`, `break`, and `continue` statements that leave a `finally` block. |  |  | 100% |
| [es-no-unsafe-function-type](es-no-unsafe-function-type.md) | Reports the built-in `Function` type, which accepts functions without checking their parameters or return values. |  |  | 100% |
| [es-no-unsafe-member-access](es-no-unsafe-member-access.md) | Reports property access on `any` values and computed keys typed as `any`. | yes |  | 97.2% |
| [es-no-unsafe-negation](es-no-unsafe-negation.md) | Reports a negation applied directly to the left operand of `in` or `instanceof`. |  |  | 100% |
| [es-no-unsafe-optional-chaining](es-no-unsafe-optional-chaining.md) | Reports optional chains used where an `undefined` result would cause an error. |  |  | 100% |
| [es-no-unsafe-return](es-no-unsafe-return.md) | Reports unsafe `any` return values, including arrays and promises. | yes |  | 98.4% |
| [es-no-unsafe-type-assertion](es-no-unsafe-type-assertion.md) | Reports type assertions that narrow a value's type without a runtime check, including unsafe assertions involving `any`. | yes |  | 94.1% |
| [es-no-unsafe-unary-minus](es-no-unsafe-unary-minus.md) | Reports unary `-` applied to values that are not numeric. | yes |  | 100% |
| [es-no-unused-expressions](es-no-unused-expressions.md) | Reports expression statements whose values are discarded without a useful effect. |  |  | 100% |
| [es-no-unused-imports](es-no-unused-imports.md) | Reports imported names that are never used. |  |  |  |
| [es-no-unused-labels](es-no-unused-labels.md) | Reports labels that no `break` or `continue` refers to. |  | autofix | 100% |
| [es-no-unused-private-class-members](es-no-unused-private-class-members.md) | Reports private fields and methods whose values are never read, and private accessors that are never accessed. |  |  | 100% |
| [es-no-unused-vars](es-no-unused-vars.md) | Reports bindings that are declared or assigned but never usefully read. |  | autofix | 99.0% |
| [es-no-use-before-define](es-no-use-before-define.md) | Reports references that appear before their declarations. |  |  | 99.8% |
| [es-no-useless-assignment](es-no-useless-assignment.md) | Reports assignments whose stored values are never read later. |  |  | 95.9% |
| [es-no-useless-backreference](es-no-useless-backreference.md) | Reports regular-expression backreferences that can only match the empty string. |  |  | 100% |
| [es-no-useless-call](es-no-useless-call.md) | Reports `.call()` and `.apply()` uses that can be replaced with a direct call without changing the intended `this` value. |  |  | 100% |
| [es-no-useless-catch](es-no-useless-catch.md) | Reports catch clauses that only rethrow the same error. |  |  | 100% |
| [es-no-useless-computed-key](es-no-useless-computed-key.md) | Reports literal property keys wrapped in unnecessary square brackets. |  | autofix | 100% |
| [es-no-useless-concat](es-no-useless-concat.md) | Reports adjacent string or template literals joined with `+` when one literal would do. |  |  | 100% |
| [es-no-useless-constructor](es-no-useless-constructor.md) | Reports constructors that add nothing to a class's default construction behaviour. |  |  | 100% |
| [es-no-useless-default-assignment](es-no-useless-default-assignment.md) | Reports default values that cannot be used because the incoming value cannot be `undefined`, and defaults that merely supply `undefined`. | yes |  | 100% |
| [es-no-useless-empty-export](es-no-useless-empty-export.md) | Reports empty exports in files already made into modules by another import or export. |  | autofix | 100% |
| [es-no-useless-escape](es-no-useless-escape.md) | Reports escapes that do not change a string, template literal, or regular expression's meaning. |  |  | 100% |
| [es-no-useless-rename](es-no-useless-rename.md) | Reports imports, exports, and destructuring bindings that rename a name to itself. |  | autofix | 100% |
| [es-no-useless-return](es-no-useless-return.md) | Reports bare `return;` statements that do not change a function's control flow. |  | autofix | 100% |
| [es-no-var-requires](es-no-var-requires.md) | Reports `require()` calls used to obtain a module value outside an import declaration. |  |  |  |
| [es-no-var](es-no-var.md) | Reports `var` declarations. |  | autofix | 100% |
| [es-no-void](es-no-void.md) | Reports the `void` operator, which evaluates an expression and discards its result. |  |  | 100% |
| [es-no-warning-comments](es-no-warning-comments.md) | Reports comments beginning with warning terms such as `TODO` or `FIXME`. |  |  | 96.7% |
| [es-no-whitespace-before-property](es-no-whitespace-before-property.md) | Reports spaces around a property-access dot or before a computed-property bracket on the same line. |  | autofix | 100% |
| [es-no-with](es-no-with.md) | Reports `with` statements. |  |  | 100% |
| [es-no-wrapper-object-types](es-no-wrapper-object-types.md) | Reports global wrapper types such as `String`, `Number` and `Boolean`. |  | autofix | 100% |
| [es-non-nullable-type-assertion-style](es-non-nullable-type-assertion-style.md) | Reports type assertions that only remove `null` and `undefined`. | yes | autofix | 84.6% |
| [es-nonblock-statement-body-position](es-nonblock-statement-body-position.md) | Controls whether a single-statement body without braces sits beside or below its condition. |  | autofix | 100% |
| [es-object-curly-spacing](es-object-curly-spacing.md) | Controls spaces inside object-like braces, including destructuring, named imports and TypeScript type bodies. |  | autofix |  |
| [es-object-shorthand](es-object-shorthand.md) | Controls shorthand properties and methods in object literals. |  | autofix | 100% |
| [es-one-var-declaration-per-line](es-one-var-declaration-per-line.md) | Reports variable declarators that should start on separate lines. |  | autofix | 100% |
| [es-one-var](es-one-var.md) | Controls whether variable declarations are combined or separated within a scope. |  | autofix | 100% |
| [es-only-throw-error](es-only-throw-error.md) | Reports thrown values whose types are not `Error` objects or allowed exceptions. | yes |  | 97.8% |
| [es-operator-assignment](es-operator-assignment.md) | Requires or forbids compound arithmetic and bitwise assignments such as `+=`. |  | autofix | 100% |
| [es-operator-linebreak](es-operator-linebreak.md) | Controls which side of an operator a line break appears on. |  | autofix | 100% |
| [es-padded-blocks](es-padded-blocks.md) | Controls blank lines just inside blocks, classes and switches. |  | autofix | 100% |
| [es-padding-line-between-statements](es-padding-line-between-statements.md) | Requires or forbids blank lines between selected kinds of statements. |  | autofix | 100% |
| [es-parameter-properties](es-parameter-properties.md) | Controls TypeScript constructor parameter properties. |  |  | 100% |
| [es-prefer-arrow-callback](es-prefer-arrow-callback.md) | Reports function expressions used as callbacks when an arrow function can replace them without changing their meaning. |  | autofix | 100% |
| [es-prefer-as-const](es-prefer-as-const.md) | Reports repeated literal types that can be expressed with a const assertion. |  | autofix | 100% |
| [es-prefer-const](es-prefer-const.md) | Reports `let` bindings that are assigned once and never reassigned. |  | autofix | 97.8% |
| [es-prefer-destructuring](es-prefer-destructuring.md) | Reports property or array-element reads that can be written as destructuring declarations or assignments. | yes | autofix | 100% |
| [es-prefer-enum-initializers](es-prefer-enum-initializers.md) | Reports enum members without explicit values. |  |  | 100% |
| [es-prefer-exponentiation-operator](es-prefer-exponentiation-operator.md) | Reports calls to `Math.pow` that can use the `**` operator. |  | autofix | 100% |
| [es-prefer-find](es-prefer-find.md) | Reports filtering an array just to take its first matching element. | yes |  | 100% |
| [es-prefer-for-of](es-prefer-for-of.md) | Reports index-based loops whose index is used only to read the current element. |  |  | 97.1% |
| [es-prefer-function-type](es-prefer-function-type.md) | Reports interfaces and object type literals containing only one call signature when a function type can express the same contract. |  | autofix | 100% |
| [es-prefer-includes](es-prefer-includes.md) | Reports membership checks written as `indexOf` comparisons or simple regular expression tests. | yes | autofix | 97.6% |
| [es-prefer-literal-enum-member](es-prefer-literal-enum-member.md) | Requires explicit enum values to be string or number literals. |  |  | 100% |
| [es-prefer-named-capture-group](es-prefer-named-capture-group.md) | Reports unnamed capturing groups in regular expressions. |  |  | 100% |
| [es-prefer-namespace-keyword](es-prefer-namespace-keyword.md) | Requires `namespace` rather than the older `module` keyword for TypeScript namespaces. |  | autofix | 100% |
| [es-prefer-nullish-coalescing](es-prefer-nullish-coalescing.md) | Reports default-value expressions that can use `??` or `??=`. | yes |  | 100% |
| [es-prefer-numeric-literals](es-prefer-numeric-literals.md) | Reports `parseInt` calls that convert a fixed binary, octal, or hexadecimal string. |  | autofix | 100% |
| [es-prefer-object-has-own](es-prefer-object-has-own.md) | Reports indirect `hasOwnProperty` calls that can use `Object.hasOwn`. |  | autofix | 100% |
| [es-prefer-object-spread](es-prefer-object-spread.md) | Reports `Object.assign` calls whose first argument is an object literal. |  | autofix | 99.1% |
| [es-prefer-optional-chain](es-prefer-optional-chain.md) | Reports repeated nullish guards that can use optional chaining. | yes | autofix | 98.8% |
| [es-prefer-promise-reject-errors](es-prefer-promise-reject-errors.md) | Requires Promise rejection reasons to be errors. | yes |  | 100% |
| [es-prefer-readonly-parameter-types](es-prefer-readonly-parameter-types.md) | Requires parameter types to be deeply readonly. | yes |  | 99.2% |
| [es-prefer-readonly](es-prefer-readonly.md) | Requires private members to be `readonly` when they are not reassigned outside the constructor. | yes | autofix | 100% |
| [es-prefer-reduce-type-parameter](es-prefer-reduce-type-parameter.md) | Reports assertions on the initial value passed to `Array#reduce`. | yes | autofix | 100% |
| [es-prefer-regex-literals](es-prefer-regex-literals.md) | Reports `RegExp` constructors with fixed patterns and flags. |  |  | 100% |
| [es-prefer-regexp-exec](es-prefer-regexp-exec.md) | Prefers `RegExp#exec` to `String#match` when the expression has no global flag. | yes | autofix | 100% |
| [es-prefer-rest-params](es-prefer-rest-params.md) | Reports use of the implicit `arguments` object. |  |  | 100% |
| [es-prefer-return-this-type](es-prefer-return-this-type.md) | Prefers the `this` return type when a class member returns its own instance. | yes | autofix | 100% |
| [es-prefer-spread](es-prefer-spread.md) | Reports `.apply()` calls that can use argument spread without changing the receiver. |  |  | 100% |
| [es-prefer-string-starts-ends-with](es-prefer-string-starts-ends-with.md) | Reports string boundary checks that can use `startsWith` or `endsWith`. | yes | autofix | 100% |
| [es-prefer-template](es-prefer-template.md) | Reports string concatenation with non-literal values. |  | autofix | 100% |
| [es-preserve-caught-error](es-preserve-caught-error.md) | Requires new errors thrown inside a catch block to preserve the caught error as their `cause`. |  |  | 100% |
| [es-promise-function-async](es-promise-function-async.md) | Requires Promise-returning functions to be `async`. | yes | autofix | 100% |
| [es-quote-props](es-quote-props.md) | Enforces a consistent policy for quoting property names. |  | autofix | 100% |
| [es-quotes](es-quotes.md) | Enforces one string delimiter style: double quotes by default. |  | autofix | 100% |
| [es-radix](es-radix.md) | Requires an explicit radix for `parseInt` and rejects invalid radix values. |  |  | 100% |
| [es-related-getter-setter-pairs](es-related-getter-setter-pairs.md) | Requires a getter's return type to be assignable to the corresponding setter's parameter type. | yes |  | 100% |
| [es-require-array-sort-compare](es-require-array-sort-compare.md) | Requires a comparison function for array `sort` and `toSorted` calls. | yes |  | 100% |
| [es-require-atomic-updates](es-require-atomic-updates.md) | Reports updates that read a shared value before `await` or `yield` and write it afterwards. |  |  | 100% |
| [es-require-await](es-require-await.md) | Reports async functions that neither await work nor return promises. | yes |  | 100% |
| [es-require-unicode-regexp](es-require-unicode-regexp.md) | Requires regular expressions to use Unicode mode with a `u` or `v` flag. |  |  | 100% |
| [es-require-yield](es-require-yield.md) | Reports nonempty generator functions that never yield a value. |  |  | 100% |
| [es-rest-spread-spacing](es-rest-spread-spacing.md) | Checks spacing between `...` and a rest or spread expression. |  | autofix | 100% |
| [es-restrict-plus-operands](es-restrict-plus-operands.md) | Checks the types used with `+` and `+=` to catch accidental coercion and incompatible addition. | yes |  | 100% |
| [es-restrict-template-expressions](es-restrict-template-expressions.md) | Checks values interpolated into template strings to avoid surprising text such as `[object Object]`. | yes |  | 100% |
| [es-return-await](es-return-await.md) | Checks whether promises returned from async functions should be awaited. | yes | autofix | 100% |
| [es-semi-spacing](es-semi-spacing.md) | Controls spaces before and after semicolons. |  | autofix | 100% |
| [es-semi-style](es-semi-style.md) | Controls which line holds a semicolon separating statements. |  | autofix | 100% |
| [es-semi](es-semi.md) | Enforces consistent statement-ending semicolons. |  | autofix | 99.7% |
| [es-sort-imports](es-sort-imports.md) | Checks import declaration order and alphabetical order of named import members. |  | autofix (members only) | 100% |
| [es-sort-keys](es-sort-keys.md) | Requires object keys in alphabetical order. |  |  | 100% |
| [es-sort-vars](es-sort-vars.md) | Requires variables within one declaration to be in alphabetical order. |  | autofix | 100% |
| [es-space-before-blocks](es-space-before-blocks.md) | Requires or disallows spaces before block-opening braces. |  | autofix | 100% |
| [es-space-before-function-paren](es-space-before-function-paren.md) | Controls spacing before function parameter lists. |  | autofix | 99.1% |
| [es-space-in-parens](es-space-in-parens.md) | Controls spaces immediately inside parentheses. |  | autofix | 100% |
| [es-space-infix-ops](es-space-infix-ops.md) | Requires spaces around infix operators, assignments and conditional-expression punctuation. |  | autofix | 100% |
| [es-space-unary-ops](es-space-unary-ops.md) | Controls spaces beside unary operators. |  | autofix | 100% |
| [es-spaced-comment](es-spaced-comment.md) | Controls whitespace after line and block comment openers. |  | autofix | 99.0% |
| [es-strict-boolean-expressions](es-strict-boolean-expressions.md) | Reports unsafe types in boolean conditions, logical negations, assertion arguments and array predicates. | yes |  | 98.2% |
| [es-strict-void-return](es-strict-void-return.md) | Reports value-returning functions used where a void-returning callback is expected. | yes |  | 96.8% |
| [es-strict](es-strict.md) | Requires or disallows `"use strict"` directives. |  | autofix | 99.2% |
| [es-switch-colon-spacing](es-switch-colon-spacing.md) | Controls spaces around `case` and `default` colons when the adjacent tokens are on the same line. |  | autofix | 100% |
| [es-switch-exhaustiveness-check](es-switch-exhaustiveness-check.md) | Reports missing cases when switching over a union of literal types or an enum. | yes |  | 100% |
| [es-symbol-description](es-symbol-description.md) | Requires a description when creating a symbol. |  |  | 100% |
| [es-template-curly-spacing](es-template-curly-spacing.md) | Controls spaces inside template substitutions. |  | autofix | 100% |
| [es-template-tag-spacing](es-template-tag-spacing.md) | Controls whitespace between a template tag and its template literal. |  | autofix (except comment-separated tags) | 100% |
| [es-triple-slash-reference](es-triple-slash-reference.md) | Controls TypeScript triple-slash references, preferring imports where possible. |  |  | 100% |
| [es-type-annotation-spacing](es-type-annotation-spacing.md) | Controls spaces around TypeScript type annotation punctuation. |  | autofix | 100% |
| [es-type-generic-spacing](es-type-generic-spacing.md) | Keeps spacing around TypeScript generic brackets consistent. |  | autofix | 100% |
| [es-type-named-tuple-spacing](es-type-named-tuple-spacing.md) | Requires named tuple members to use `name: Type` or `name?: Type`. |  | autofix | 100% |
| [es-unbound-method](es-unbound-method.md) | Reports methods extracted from their object without binding their receiver. | yes |  | 98.6% |
| [es-unicode-bom](es-unicode-bom.md) | Controls whether a file starts with the Unicode byte order mark, U+FEFF. |  | autofix | 100% |
| [es-unified-signatures](es-unified-signatures.md) | Reports overloads that could be combined using a union, an optional parameter or a rest parameter. |  |  | 100% |
| [es-use-isnan](es-use-isnan.md) | Reports comparisons with `NaN`, including `Number.NaN`. |  |  | 100% |
| [es-use-unknown-in-catch-callback-variable](es-use-unknown-in-catch-callback-variable.md) | Requires Promise rejection callback parameters to have type `unknown` rather than `any` or a narrower assumed error type. | yes |  | 100% |
| [es-valid-typeof](es-valid-typeof.md) | Reports comparisons between `typeof` results and invalid type names. |  |  | 100% |
| [es-vars-on-top](es-vars-on-top.md) | Requires `var` declarations to appear together at the start of their function, program or class static block. |  |  | 100% |
| [es-wrap-iife](es-wrap-iife.md) | Requires parentheses around immediately invoked function expressions. |  | autofix | 100% |
| [es-wrap-regex](es-wrap-regex.md) | Requires parentheses around regular expression literals used as the object of a property access. |  | autofix | 100% |
| [es-yield-star-spacing](es-yield-star-spacing.md) | Keeps spacing around the star in delegated `yield*` expressions consistent. |  | autofix | 100% |
| [es-yoda](es-yoda.md) | Controls which side of a comparison holds a literal value. |  | autofix | 100% |
| [rs-array-spacing](rs-array-spacing.md) | Requires spaces just inside nonempty single-line Rust array literals. |  | autofix |  |
| [rs-attribute-spacing](rs-attribute-spacing.md) | Removes whitespace between an attribute's `#` or `#!` and its opening bracket. |  | autofix |  |
| [rs-brace-spacing](rs-brace-spacing.md) | Requires spacing before opening braces and inside single-line braces. |  | autofix |  |
| [rs-bracket-spacing](rs-bracket-spacing.md) | Forbids spaces just inside Rust index brackets. |  | autofix |  |
| [rs-call-spacing](rs-call-spacing.md) | Forbids spaces before call parentheses and index brackets. |  | autofix |  |
| [rs-closure-spacing](rs-closure-spacing.md) | Keeps closure parameters tight against their pipes and requires whitespace before the body. |  | autofix |  |
| [rs-colon-spacing](rs-colon-spacing.md) | Forbids spaces before a colon and requires whitespace after it. |  | autofix |  |
| [rs-comma-spacing](rs-comma-spacing.md) | Forbids spaces before commas and requires whitespace after a comma followed by another item. |  | autofix |  |
| [rs-dot-spacing](rs-dot-spacing.md) | Removes horizontal whitespace around dots used for Rust field and method access. |  | autofix |  |
| [rs-eol-last](rs-eol-last.md) | Requires nonempty Rust files to end with one line break and removes extra blank lines at the end. |  | autofix |  |
| [rs-fn-parenthesis-spacing](rs-fn-parenthesis-spacing.md) | Requires a space between a function declaration's name and its parameter list. |  | autofix |  |
| [rs-generic-spacing](rs-generic-spacing.md) | Removes spaces immediately after a generic argument or parameter list's opening `<`. |  | autofix |  |
| [rs-indent](rs-indent.md) | Checks block and continuation indentation in Rust. |  | autofix |  |
| [rs-keyword-spacing](rs-keyword-spacing.md) | Requires whitespace after Rust keywords before expressions, and before `else`, `as`, `in` or `where` following a closing delimiter. |  | autofix |  |
| [rs-no-blank-before-close](rs-no-blank-before-close.md) | Removes blank lines immediately before a closing brace or square bracket. |  | autofix |  |
| [rs-no-multi-spaces](rs-no-multi-spaces.md) | Replaces runs of two or more horizontal whitespace characters between Rust code tokens with one space. |  | autofix |  |
| [rs-no-multiple-empty-lines](rs-no-multiple-empty-lines.md) | Limits consecutive blank lines in Rust code to one. |  | autofix |  |
| [rs-no-trailing-spaces](rs-no-trailing-spaces.md) | Removes horizontal whitespace at the ends of Rust code lines and on otherwise blank lines. |  | autofix |  |
| [rs-operator-spacing](rs-operator-spacing.md) | Adds missing spaces around Rust binary operators, assignment operators and arrows. |  | autofix |  |
| [rs-parenthesis-spacing](rs-parenthesis-spacing.md) | Forbids horizontal whitespace just inside nonempty Rust parentheses. |  | autofix |  |
| [rs-path-spacing](rs-path-spacing.md) | Removes horizontal whitespace around Rust path separators (`::`). |  | autofix |  |
| [rs-semi-spacing](rs-semi-spacing.md) | Disallows horizontal whitespace before Rust semicolons. |  | autofix |  |
| [rs-try-spacing](rs-try-spacing.md) | Disallows horizontal whitespace before Rust's `?` operator. |  | autofix |  |
| [rs-unary-spacing](rs-unary-spacing.md) | Disallows horizontal whitespace after Rust unary operators and reference markers. |  | autofix |  |
| [rs-use-list-breaks](rs-use-list-breaks.md) | Requires one item per line in Rust `use` lists containing paths, globs or nested lists. |  |  |  |
| [shared-blank-lines](shared-blank-lines.md) | Limits consecutive blank lines. |  | autofix |  |
| [shared-eol-last](shared-eol-last.md) | Require a final newline in non-empty files, or forbid final newlines. |  | autofix | 90.9% |
| [shared-final-newline](shared-final-newline.md) | Require exactly one final newline in non-empty files. |  | autofix |  |
| [shared-linebreak-style](shared-linebreak-style.md) | Reports line endings that differ from the selected style. |  | autofix | 92.9% |
| [shared-max-line-length](shared-max-line-length.md) | Reports text lines longer than the configured character limit. |  |  |  |
| [shared-no-trailing-spaces](shared-no-trailing-spaces.md) | Reports whitespace at the end of a line. |  | autofix | 100% |
| [shared-tab-indent](shared-tab-indent.md) | Reports tabs in leading indentation and replaces each tab with a fixed number of spaces. |  | autofix |  |
| [shared-trailing-whitespace](shared-trailing-whitespace.md) | Removes spaces and tabs at line ends, including whitespace-only lines. |  | autofix |  |
