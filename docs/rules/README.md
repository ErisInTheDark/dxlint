# Rules

Every rule dxlint ships, 433 in all. Turn one on with `rule <name>` after importing its language; see [basic.md](../basic.md).

- **Types**: the rule reads type information (from your `tsconfig.json`; TypeScript itself never runs).
- **Fix**: `autofix` rules are fixed by `dxlint --fix` and by the editor's quick fixes.
- **Parity**: the share of the upstream rule's own test cases where dxlint gives the same result. 337 of the 391 measured rules match every case; 43,352 of 43,622 cases match overall. Each page lists its known differences.

Pages are adapted from the documentation of [ESLint](https://eslint.org/docs/latest/rules/), [typescript-eslint](https://typescript-eslint.io/rules/), [ESLint Stylistic](https://eslint.style/rules) and [eslint-plugin-import](https://github.com/import-js/eslint-plugin-import), all MIT licensed.

| Rule | What it reports | Types | Fix | Parity |
| --- | --- | --- | --- | --- |
| [accessor-pairs](accessor-pairs.md) | Reports properties with a setter but no getter. |  |  | 100% |
| [adjacent-overload-signatures](adjacent-overload-signatures.md) | Requires overload signatures for the same function or method to be next to each other. |  |  | 100% |
| [array-bracket-spacing](array-bracket-spacing.md) | Enforces spaces inside array brackets, including destructuring patterns. |  | autofix |  |
| [array-callback-return](array-callback-return.md) | Reports array callbacks that fail to return a value where one is needed, such as callbacks to `map`, `filter` and `reduce`. |  |  | 100% |
| [array-type](array-type.md) | Enforces one notation for TypeScript array types: `T[]` or `Array<T>`. |  | autofix | 100% |
| [arrow-body-style](arrow-body-style.md) | Controls whether arrow functions use block bodies or return expressions directly. |  | autofix | 100% |
| [arrow-parens](arrow-parens.md) | Enforces consistent parentheses around arrow parameters. |  | autofix | 93.6% |
| [arrow-spacing](arrow-spacing.md) | Controls spaces before and after an arrow function's `=>`. |  | autofix | 100% |
| [await-thenable](await-thenable.md) | Reports awaiting values that are not promises or other thenables. | yes |  | 97.7% |
| [ban-ts-comment](ban-ts-comment.md) | Controls TypeScript directive comments that suppress or enable compiler checks. |  |  | 100% |
| [ban-tslint-comment](ban-tslint-comment.md) | Reports obsolete TSLint enable and disable comments. |  | autofix | 100% |
| [blank-lines](blank-lines.md) | Limits consecutive blank lines. |  | autofix |  |
| [block-scoped-var](block-scoped-var.md) | Reports `var` bindings used outside the block where they were declared. |  |  | 100% |
| [block-spacing](block-spacing.md) | Controls spaces immediately inside block braces when the next or previous token is on the same line. |  | autofix | 100% |
| [brace-style](brace-style.md) | Enforces consistent block brace placement. |  | autofix | 100% |
| [camelcase](camelcase.md) | Reports names with underscores between words instead of camel case. |  |  | 100% |
| [capitalized-comments](capitalized-comments.md) | Enforces the initial letter case of comments. |  | autofix | 96.6% |
| [check-alignment](check-alignment.md) | Reports misaligned JSDoc blocks. |  |  |  |
| [class-literal-property-style](class-literal-property-style.md) | Enforces one way to expose constant literal values on classes: readonly fields or getters. |  |  | 100% |
| [class-methods-use-this](class-methods-use-this.md) | Reports instance methods and function-valued fields that never use `this`. |  |  | 100% |
| [comma-dangle](comma-dangle.md) | Controls trailing commas in lists, including arrays, objects, calls, imports and TypeScript declarations. |  | autofix | 97.5% |
| [comma-spacing](comma-spacing.md) | Enforces spaces around commas. |  | autofix | 98.9% |
| [comma-style](comma-style.md) | Controls whether a comma belongs at the end of its item line or the start of the following line. |  | autofix | 100% |
| [complexity](complexity.md) | Limits cyclomatic complexity: the number of independent paths through a function. |  |  | 100% |
| [computed-property-spacing](computed-property-spacing.md) | Controls spaces inside computed property brackets, such as `object[key]` and `{ [key]: value }`. |  | autofix | 100% |
| [consistent-generic-constructors](consistent-generic-constructors.md) | Enforces one location for generic arguments when constructing a value: the type annotation or the constructor call. |  | autofix | 100% |
| [consistent-indexed-object-style](consistent-indexed-object-style.md) | Enforces one form for dictionary-like types: `Record` or an index signature. |  | autofix | 98.2% |
| [consistent-return](consistent-return.md) | Requires return statements in a function to consistently return a value or omit it. | yes |  | 100% |
| [consistent-this](consistent-this.md) | Require consistent names for variables that capture `this`. |  |  | 100% |
| [consistent-type-assertions](consistent-type-assertions.md) | Use one type assertion style throughout the project. |  | autofix | 100% |
| [consistent-type-definitions](consistent-type-definitions.md) | Use either interfaces or type aliases consistently for object shapes. |  | autofix | 100% |
| [consistent-type-exports](consistent-type-exports.md) | Mark exports that exist only in the type system with `type`. | yes | autofix | 100% |
| [consistent-type-imports](consistent-type-imports.md) | Use explicit type imports for names used only as types. |  | autofix | 100% |
| [constructor-super](constructor-super.md) | Require valid `super()` calls in constructors. |  |  | 100% |
| [curly](curly.md) | Require a consistent brace policy for conditional and loop bodies. |  | autofix | 100% |
| [default-case-last](default-case-last.md) | Put a switch's `default` clause after all its cases. |  |  | 100% |
| [default-case](default-case.md) | Require a `default` clause in switch statements, or a comment explaining its absence. |  |  | 100% |
| [default-param-last](default-param-last.md) | Put defaulted and optional parameters after required parameters. |  |  | 100% |
| [dot-location](dot-location.md) | Keep member-access dots on a consistent side of a line break. |  | autofix | 100% |
| [dot-notation](dot-notation.md) | Prefer dot notation when a property can be named directly. | yes | autofix | 100% |
| [eol-last](eol-last.md) | Require a final newline in non-empty files, or forbid final newlines. |  | autofix | 90.9% |
| [eqeqeq](eqeqeq.md) | Prefer strict equality operators so comparisons do not silently coerce values. |  | autofix | 100% |
| [explicit-function-return-type](explicit-function-return-type.md) | Require return type annotations on functions and methods. |  |  | 100% |
| [explicit-member-accessibility](explicit-member-accessibility.md) | Require explicit visibility on class members, or forbid redundant `public` modifiers. |  | autofix | 100% |
| [explicit-module-boundary-types](explicit-module-boundary-types.md) | Require argument and return types on exported functions and public methods of exported classes. |  |  | 100% |
| [final-newline](final-newline.md) | Require exactly one final newline in non-empty files. |  | autofix |  |
| [for-direction](for-direction.md) | Report for-loop counters that move away from their stop condition. |  |  | 100% |
| [func-name-matching](func-name-matching.md) | Match a named function expression's name to its variable or property name. |  |  | 100% |
| [func-names](func-names.md) | Require or forbid names on function expressions. |  |  | 100% |
| [func-style](func-style.md) | Choose function declarations or expressions for functions assigned to names. |  |  | 100% |
| [function-call-spacing](function-call-spacing.md) | Require or forbid whitespace before invocation parentheses. |  | autofix | 100% |
| [generator-star-spacing](generator-star-spacing.md) | Keep spacing around generator stars consistent. |  | autofix | 100% |
| [getter-return](getter-return.md) | Require getters to return a value on paths that finish normally. |  |  | 100% |
| [grouped-accessor-pairs](grouped-accessor-pairs.md) | Keep each property's getter and setter next to each other. |  |  | 100% |
| [guard-for-in](guard-for-in.md) | Require a guard inside for-in loops to filter unwanted properties. |  |  | 100% |
| [header](header.md) | Require a configured licence comment at the start of a file. |  | autofix |  |
| [id-denylist](id-denylist.md) | Reports identifiers on a list of forbidden names. |  |  | 100% |
| [id-length](id-length.md) | Reports identifier names shorter or longer than the configured limits. |  |  | 100% |
| [id-match](id-match.md) | Reports identifier names that do not match a regular expression. |  |  | 100% |
| [implicit-arrow-linebreak](implicit-arrow-linebreak.md) | Requires a consistent position for an arrow function's implicitly returned expression. |  | autofix | 100% |
| [import-no-deprecated](import-no-deprecated.md) | Reports imports and uses of exports marked with JSDoc `@deprecated`. | yes |  |  |
| [indent-binary-ops](indent-binary-ops.md) | Reports uneven indentation in multiline binary expressions. |  | autofix | 100% |
| [indent](indent.md) | Reports inconsistent leading indentation in JavaScript and TypeScript. |  | autofix | 98.5% |
| [init-declarations](init-declarations.md) | Requires variables to be initialised when declared, or forbids initialisers with `"never"`. |  |  | 100% |
| [jsx-child-element-spacing](jsx-child-element-spacing.md) | Reports ambiguous spacing between JSX text and inline elements on neighbouring lines. |  |  | 100% |
| [jsx-closing-bracket-location](jsx-closing-bracket-location.md) | Checks where the final `>` or `/>` of a multiline JSX opening tag appears. |  | autofix | 100% |
| [jsx-closing-tag-location](jsx-closing-tag-location.md) | Requires the closing tag of multiline JSX to be on its own line and correctly aligned. |  | autofix | 100% |
| [jsx-curly-brace-presence](jsx-curly-brace-presence.md) | Reports unnecessary braces around JSX string literals, or requires braces when configured. |  | autofix | 99.3% |
| [jsx-curly-newline](jsx-curly-newline.md) | Checks line breaks directly inside JSX expression braces. |  | autofix | 100% |
| [jsx-curly-spacing](jsx-curly-spacing.md) | Requires or forbids spaces inside JSX expression braces. |  | autofix | 100% |
| [jsx-equals-spacing](jsx-equals-spacing.md) | Requires consistent spaces around `=` in JSX attributes. |  | autofix | 100% |
| [jsx-first-prop-new-line](jsx-first-prop-new-line.md) | Checks whether the first JSX prop starts beside the tag name or on a new line. |  | autofix | 100% |
| [jsx-function-call-newline](jsx-function-call-newline.md) | Requires line breaks around JSX used as a function argument. |  | autofix | 100% |
| [jsx-indent-props](jsx-indent-props.md) | Reports inconsistent indentation of JSX props that start a new line. |  | autofix | 100% |
| [jsx-max-props-per-line](jsx-max-props-per-line.md) | Limits how many JSX props can share a line. |  | autofix | 100% |
| [jsx-newline](jsx-newline.md) | Requires or prevents blank lines between adjacent JSX elements and expressions. |  | autofix | 100% |
| [jsx-one-expression-per-line](jsx-one-expression-per-line.md) | Requires JSX children to occupy separate lines rather than sharing a line with other children or their parent tags. |  | autofix | 100% |
| [jsx-pascal-case](jsx-pascal-case.md) | Reports user-defined JSX component names that are not PascalCase. |  |  | 100% |
| [jsx-props-style](jsx-props-style.md) | Keeps JSX prop wrapping consistent with the first prop's position. |  | autofix | 100% |
| [jsx-quotes](jsx-quotes.md) | Requires consistent quote marks around JSX attribute strings. |  | autofix | 100% |
| [jsx-self-closing-comp](jsx-self-closing-comp.md) | Reports JSX elements with an unnecessary separate closing tag because they have no children. |  | autofix | 100% |
| [jsx-shorthand-boolean](jsx-shorthand-boolean.md) | Reports JSX attributes explicitly set to `true`. |  | autofix | 100% |
| [jsx-shorthand-fragment](jsx-shorthand-fragment.md) | Prefers `<>...</>` over `Fragment` and `React.Fragment` when no props are needed. |  | autofix | 100% |
| [jsx-tag-spacing](jsx-tag-spacing.md) | Checks spaces beside JSX angle brackets and slashes. |  | autofix | 100% |
| [jsx-wrap-multilines](jsx-wrap-multilines.md) | Reports multiline JSX without the configured parentheses. |  | autofix | 100% |
| [key-spacing](key-spacing.md) | Reports inconsistent spaces around property colons. |  | autofix | 100% |
| [keyword-spacing](keyword-spacing.md) | Reports missing or unwanted spaces around keywords. |  | autofix | 100% |
| [line-comment-position](line-comment-position.md) | Reports line comments in the wrong position: beside code or on their own line. |  |  | 86.1% |
| [linebreak-style](linebreak-style.md) | Reports line endings that differ from the selected style. |  | autofix | 92.9% |
| [lines-around-comment](lines-around-comment.md) | Reports missing blank lines around standalone comments. |  | autofix | 100% |
| [lines-between-class-members](lines-between-class-members.md) | Reports missing or unwanted blank lines between class members. |  | autofix | 100% |
| [list-style](list-style.md) | Reports inconsistent spacing and wrapping inside brackets. |  | autofix | 100% |
| [logical-assignment-operators](logical-assignment-operators.md) | Reports assignments that can use `\|\|=`, `&&=`, or `??=`. |  | autofix | 100% |
| [max-classes-per-file](max-classes-per-file.md) | Reports files containing more classes than the configured limit. |  |  | 94.7% |
| [max-depth](max-depth.md) | Reports control-flow blocks nested beyond the limit. |  |  | 100% |
| [max-len](max-len.md) | Reports lines longer than the configured limit. |  |  | 100% |
| [max-line-length](max-line-length.md) | Reports text lines longer than the configured character limit. |  |  |  |
| [max-lines-per-function](max-lines-per-function.md) | Reports functions with too many lines. |  |  | 100% |
| [max-lines](max-lines.md) | Reports files with too many lines. |  |  | 100% |
| [max-nested-callbacks](max-nested-callbacks.md) | Reports callbacks nested beyond the configured limit. |  |  | 100% |
| [max-params](max-params.md) | Reports functions with too many parameters. |  |  | 100% |
| [max-statements-per-line](max-statements-per-line.md) | Reports lines containing too many statements. |  |  | 100% |
| [max-statements](max-statements.md) | Reports functions containing too many statements. |  |  | 100% |
| [member-delimiter-style](member-delimiter-style.md) | Reports inconsistent separators between interface and type literal members. |  | autofix | 100% |
| [member-ordering](member-ordering.md) | Reports members declared out of order in classes, interfaces, and type literals. |  |  | 100% |
| [method-signature-style](method-signature-style.md) | Reports function members written in the wrong signature style. |  | autofix | 100% |
| [multiline-comment-style](multiline-comment-style.md) | Reports multiline comments that do not follow the selected shape. |  | autofix | 95.5% |
| [multiline-ternary](multiline-ternary.md) | Reports missing or unwanted line breaks between conditional expression operands. |  | autofix | 100% |
| [naming-convention](naming-convention.md) | Reports names that do not follow the convention for their declaration kind. |  |  | 97.8% |
| [new-cap](new-cap.md) | Reports lowercase constructor names and capitalised functions called without `new`. |  |  | 100% |
| [new-parens](new-parens.md) | Reports missing or unwanted empty parentheses in constructor calls. |  | autofix | 100% |
| [newline-per-chained-call](newline-per-chained-call.md) | Reports chained calls that need a preceding line break. |  | autofix | 100% |
| [no-alert](no-alert.md) | Reports calls to the global `alert`, `confirm` and `prompt` functions. |  |  | 100% |
| [no-array-constructor](no-array-constructor.md) | Reports array constructor calls that can use array literals instead. |  | autofix | 100% |
| [no-array-delete](no-array-delete.md) | Reports deleting an array element. | yes |  | 100% |
| [no-async-promise-executor](no-async-promise-executor.md) | Reports async functions passed as `Promise` executors. |  |  | 100% |
| [no-await-in-loop](no-await-in-loop.md) | Reports `await` operations inside loops, including `await using`. |  |  | 100% |
| [no-base-to-string](no-base-to-string.md) | Reports string conversions that use an object's unhelpful default representation, such as `"[object Object]"`. | yes |  | 98.4% |
| [no-bitwise](no-bitwise.md) | Reports bitwise operators. |  |  | 100% |
| [no-blank-lines-in-optional-blocks](no-blank-lines-in-optional-blocks.md) | Reports blank lines between an `if` or loop header and its single unbraced statement. |  | autofix |  |
| [no-caller](no-caller.md) | Reports `arguments.caller` and `arguments.callee`. |  |  | 100% |
| [no-case-declarations](no-case-declarations.md) | Reports lexical declarations directly inside `case` or `default` clauses. |  |  | 100% |
| [no-class-assign](no-class-assign.md) | Reports assignments to class declaration bindings. |  |  | 100% |
| [no-compare-neg-zero](no-compare-neg-zero.md) | Reports comparisons against `-0`. |  |  | 100% |
| [no-cond-assign](no-cond-assign.md) | Reports assignments used as conditions where a comparison may have been intended. |  |  | 100% |
| [no-confusing-arrow](no-confusing-arrow.md) | Reports arrow functions whose conditional expression bodies can look like comparisons. |  | autofix | 100% |
| [no-confusing-non-null-assertion](no-confusing-non-null-assertion.md) | Reports non-null assertions next to assignment, equality, `in` or `instanceof` operators. |  |  | 100% |
| [no-confusing-void-expression](no-confusing-void-expression.md) | Reports `void`-typed expressions used where a value appears to be needed. | yes | autofix | 99.1% |
| [no-console](no-console.md) | Reports use of `console` methods, including assignments to them. |  |  | 100% |
| [no-const-assign](no-const-assign.md) | Reports writes to constant bindings. |  |  | 100% |
| [no-constant-binary-expression](no-constant-binary-expression.md) | Reports comparisons whose result is fixed and logical expressions that always or never short-circuit. |  |  | 100% |
| [no-constant-condition](no-constant-condition.md) | Reports conditions whose truthiness cannot change. |  |  | 100% |
| [no-constructor-return](no-constructor-return.md) | Reports return statements with values inside class constructors. |  |  | 100% |
| [no-continue](no-continue.md) | Reports `continue` statements, including labelled ones. |  |  | 100% |
| [no-control-regex](no-control-regex.md) | Reports raw ASCII control characters and hexadecimal or Unicode escapes for them in regular expressions. |  |  | 100% |
| [no-debugger](no-debugger.md) | Reports `debugger` statements. |  |  | 100% |
| [no-delete-var](no-delete-var.md) | Reports use of `delete` on a variable. |  |  | 100% |
| [no-deprecated](no-deprecated.md) | Reports references to symbols marked with a JSDoc `@deprecated` tag. | yes |  | 92.5% |
| [no-div-regex](no-div-regex.md) | Reports regular expression literals starting with `=`. |  | autofix | 100% |
| [no-dupe-args](no-dupe-args.md) | Reports repeated parameter names in function declarations and expressions. |  |  | 100% |
| [no-dupe-class-members](no-dupe-class-members.md) | Reports class members with the same name when one would overwrite another. |  |  | 100% |
| [no-dupe-else-if](no-dupe-else-if.md) | Reports conditions already covered by earlier branches in an `if` / `else if` chain. |  |  | 100% |
| [no-dupe-keys](no-dupe-keys.md) | Reports repeated keys in object literals. |  |  | 100% |
| [no-duplicate-case](no-duplicate-case.md) | Reports repeated test expressions in a `switch` statement. |  |  | 100% |
| [no-duplicate-enum-values](no-duplicate-enum-values.md) | Reports enum members explicitly assigned the same literal value. |  |  | 100% |
| [no-duplicate-imports](no-duplicate-imports.md) | Reports imports and re-exports from the same module when their declarations can be combined. |  |  | 100% |
| [no-duplicate-type-constituents](no-duplicate-type-constituents.md) | Reports repeated types in unions and intersections, including aliases that resolve to the same type. | yes | autofix | 100% |
| [no-dynamic-delete](no-dynamic-delete.md) | Reports `delete` with a computed key that is not a string or number literal. |  |  | 100% |
| [no-else-return](no-else-return.md) | Reports an `else` after a branch that always returns. |  | autofix | 100% |
| [no-empty-character-class](no-empty-character-class.md) | Reports empty character classes in regular expression literals. |  |  | 100% |
| [no-empty-function](no-empty-function.md) | Reports functions with empty bodies and no explanatory comment. |  |  | 100% |
| [no-empty-interface](no-empty-interface.md) | Reports interfaces with no members and at most one supertype. |  |  |  |
| [no-empty-object-type](no-empty-object-type.md) | Reports empty interfaces and `{}` type literals. |  |  | 100% |
| [no-empty-pattern](no-empty-pattern.md) | Reports empty destructuring patterns. |  |  | 100% |
| [no-empty-static-block](no-empty-static-block.md) | Reports empty class static blocks. |  |  | 100% |
| [no-empty](no-empty.md) | Reports empty blocks and empty `switch` statements. |  |  | 100% |
| [no-eq-null](no-eq-null.md) | Reports loose equality comparisons with `null`. |  |  | 100% |
| [no-eval](no-eval.md) | Reports calls and references to `eval`. |  |  | 100% |
| [no-ex-assign](no-ex-assign.md) | Reports assignments to a caught exception parameter. |  |  | 100% |
| [no-explicit-any](no-explicit-any.md) | Reports explicit `any` types, which bypass type checking. |  | autofix (with `fixToUnknown`) | 100% |
| [no-extend-native](no-extend-native.md) | Reports additions to built-in prototypes. |  |  | 100% |
| [no-extra-bind](no-extra-bind.md) | Reports `.bind()` calls that do not change the function's behaviour. |  | autofix | 100% |
| [no-extra-boolean-cast](no-extra-boolean-cast.md) | Reports boolean conversions in places that already convert their value to a boolean. |  | autofix | 100% |
| [no-extra-label](no-extra-label.md) | Reports labels on `break` and `continue` when the same statement would be targeted without the label. |  | autofix | 100% |
| [no-extra-non-null-assertion](no-extra-non-null-assertion.md) | Reports non-null assertions that add nothing, such as repeated `!` assertions. |  | autofix | 100% |
| [no-extra-parens](no-extra-parens.md) | Reports parentheses that do not affect an expression's meaning. |  | autofix | 100% |
| [no-extra-semi](no-extra-semi.md) | Reports unnecessary semicolons, such as repeated statement terminators or semicolons after function declarations. |  | autofix | 100% |
| [no-extraneous-class](no-extraneous-class.md) | Reports empty classes, constructor-only classes and classes used only as static namespaces. |  |  | 100% |
| [no-fallthrough](no-fallthrough.md) | Reports switch cases that run into the next case without a break or other exit. |  |  | 96.6% |
| [no-floating-decimal](no-floating-decimal.md) | Requires digits on both sides of a decimal point. |  | autofix | 100% |
| [no-floating-promises](no-floating-promises.md) | Reports promise expressions whose rejection is not handled. | yes |  | 92.3% |
| [no-for-in-array](no-for-in-array.md) | Reports `for-in` loops over arrays. | yes |  | 100% |
| [no-func-assign](no-func-assign.md) | Reports writes to function declaration names. |  |  | 100% |
| [no-generated-empty-object-type](no-generated-empty-object-type.md) | Reports type operations that resolve to the empty object type, `{}`. | yes |  | 92.6% |
| [no-global-assign](no-global-assign.md) | Reports assignments to read-only globals. |  |  | 100% |
| [no-implicit-coercion](no-implicit-coercion.md) | Reports shorthand type conversions such as `!!value`, `+value`, and `"" + value`. |  | autofix | 100% |
| [no-implicit-globals](no-implicit-globals.md) | Reports declarations and assignments that pollute script global scope. |  |  | 100% |
| [no-implied-eval](no-implied-eval.md) | Reports code evaluated from strings by timers, `execScript`, or the `Function` constructor. | yes |  | 100% |
| [no-import-assign](no-import-assign.md) | Reports writes to imported bindings and namespace import members. |  |  | 100% |
| [no-import-type-side-effects](no-import-type-side-effects.md) | Requires a top-level `import type` when every named specifier is type-only. |  | autofix | 100% |
| [no-inferrable-types](no-inferrable-types.md) | Reports type annotations already clear from an initial or default value. |  | autofix | 100% |
| [no-inline-comments](no-inline-comments.md) | Reports comments on the same line as code. |  |  | 100% |
| [no-inner-declarations](no-inner-declarations.md) | Reports function declarations, and optionally `var` declarations, nested below a program or function body. |  |  | 100% |
| [no-invalid-regexp](no-invalid-regexp.md) | Reports invalid patterns and flags passed to `RegExp`. |  |  | 100% |
| [no-invalid-this](no-invalid-this.md) | Reports `this` where it may be undefined in strict code. |  |  | 100% |
| [no-invalid-void-type](no-invalid-void-type.md) | Reports `void` types outside supported return and generic type positions. |  |  | 100% |
| [no-irregular-whitespace](no-irregular-whitespace.md) | Reports unusual whitespace characters that are easy to overlook or confuse with ordinary spaces. |  |  | 100% |
| [no-iterator](no-iterator.md) | Reports access to the obsolete SpiderMonkey `__iterator__` property. |  |  | 100% |
| [no-label-var](no-label-var.md) | Reports statement labels sharing a name with an in-scope variable. |  |  | 100% |
| [no-labels](no-labels.md) | Reports labeled statements and labeled `break` or `continue`. |  |  | 100% |
| [no-lone-blocks](no-lone-blocks.md) | Reports standalone blocks that provide no useful scope. |  |  | 100% |
| [no-lonely-if](no-lonely-if.md) | Reports an `if` that is the only statement in an `else` block. |  | autofix | 100% |
| [no-loop-func](no-loop-func.md) | Reports functions created in loops that capture variables modified outside their own scope. |  |  | 100% |
| [no-loss-of-precision](no-loss-of-precision.md) | Reports number literals whose written precision is lost when represented as a JavaScript number. |  |  | 100% |
| [no-magic-numbers](no-magic-numbers.md) | Reports numeric literals used without a meaningful name. |  |  | 100% |
| [no-meaningless-void-operator](no-meaningless-void-operator.md) | Reports `void` when it does not discard a useful call result. | yes | autofix | 95.3% |
| [no-misleading-character-class](no-misleading-character-class.md) | Reports character classes that split a visible Unicode character into separate parts. |  |  | 100% |
| [no-misused-new](no-misused-new.md) | Reports constructor-like declarations in the wrong place: a class method named `new`, an interface constructing itself, or an interface/type member named `constructor`. |  |  | 100% |
| [no-misused-promises](no-misused-promises.md) | Reports promises used where a boolean, ordinary object, or synchronous callback is expected. | yes |  | 96.4% |
| [no-misused-spread](no-misused-spread.md) | Reports spreads whose types suggest a likely mistake, such as promises, functions, maps, arrays, or class instances copied into objects. | yes |  | 99.3% |
| [no-mixed-enums](no-mixed-enums.md) | Reports enums containing both numeric and string members. | yes |  | 100% |
| [no-mixed-operators](no-mixed-operators.md) | Requires parentheses around mixtures of operators in the same configured group. |  |  | 100% |
| [no-mixed-spaces-and-tabs](no-mixed-spaces-and-tabs.md) | Reports indentation containing both tabs and spaces. |  |  | 100% |
| [no-multi-assign](no-multi-assign.md) | Reports chained assignments. |  |  | 100% |
| [no-multi-spaces](no-multi-spaces.md) | Reports repeated spaces between code tokens, rather than indentation or spaces inside strings and comments. |  | autofix | 100% |
| [no-multi-str](no-multi-str.md) | Reports quoted strings continued across physical lines with a backslash. |  |  | 100% |
| [no-multiple-empty-lines](no-multiple-empty-lines.md) | Limits runs of blank lines to reduce unnecessary scrolling. |  | autofix | 100% |
| [no-namespace](no-namespace.md) | Reports TypeScript namespaces and old-style named modules in ordinary source files. |  |  | 100% |
| [no-negated-condition](no-negated-condition.md) | Reports negated conditions in ternaries and `if` statements with an `else` branch. |  |  | 100% |
| [no-nested-ternary](no-nested-ternary.md) | Reports ternary expressions nested inside another ternary's result branches. |  |  | 100% |
| [no-new-func](no-new-func.md) | Reports use of the global `Function` constructor, with or without `new`. |  |  | 100% |
| [no-new-native-nonconstructor](no-new-native-nonconstructor.md) | Reports `new Symbol` and `new BigInt`. |  |  | 100% |
| [no-new-wrappers](no-new-wrappers.md) | Reports `new String`, `new Number`, and `new Boolean`. |  |  | 100% |
| [no-new](no-new.md) | Reports constructor calls used as standalone statements, discarding the new object. |  |  | 100% |
| [no-non-null-asserted-nullish-coalescing](no-non-null-asserted-nullish-coalescing.md) | Reports non-null assertions on the left side of `??`. |  |  | 100% |
| [no-non-null-asserted-optional-chain](no-non-null-asserted-optional-chain.md) | Reports non-null assertions applied to optional-chain results. |  |  | 100% |
| [no-non-null-assertion](no-non-null-assertion.md) | Reports TypeScript's postfix `!` non-null assertion. |  |  | 100% |
| [no-nonoctal-decimal-escape](no-nonoctal-decimal-escape.md) | Reports `\8` and `\9` in string literals. |  |  | 100% |
| [no-obj-calls](no-obj-calls.md) | Reports calls and constructor calls to built-in objects that are not functions, such as `Math` and `JSON`. |  |  | 100% |
| [no-object-constructor](no-object-constructor.md) | Reports calls to `Object` without an argument, with or without `new`. |  |  | 100% |
| [no-octal-escape](no-octal-escape.md) | Reports legacy octal escape sequences in strings. |  |  | 100% |
| [no-octal](no-octal.md) | Reports legacy octal number literals such as `071`. |  |  | 100% |
| [no-param-reassign](no-param-reassign.md) | Reports writes to function parameters. |  |  | 100% |
| [no-plusplus](no-plusplus.md) | Reports the increment and decrement operators, `++` and `--`. |  |  | 100% |
| [no-promise-executor-return](no-promise-executor-return.md) | Reports returned values in Promise executor functions. |  |  | 100% |
| [no-proto](no-proto.md) | Reports access to the legacy `__proto__` property. |  |  | 100% |
| [no-prototype-builtins](no-prototype-builtins.md) | Reports direct calls to `hasOwnProperty`, `isPrototypeOf` and `propertyIsEnumerable` on objects. |  |  | 100% |
| [no-redeclare](no-redeclare.md) | Reports names declared more than once in the same scope. |  |  | 99.2% |
| [no-redundant-type-constituents](no-redundant-type-constituents.md) | Reports union and intersection members that add no useful type information. | yes |  | 100% |
| [no-regex-spaces](no-regex-spaces.md) | Reports consecutive literal spaces in regular expressions. |  | autofix | 100% |
| [no-require-imports](no-require-imports.md) | Reports CommonJS `require()` imports in favour of ES module syntax. |  |  | 100% |
| [no-restricted-exports](no-restricted-exports.md) | Reports exported names listed in your project's restrictions. |  |  | 100% |
| [no-restricted-globals](no-restricted-globals.md) | Reports uses of configured global names. |  |  | 100% |
| [no-restricted-imports](no-restricted-imports.md) | Reports imports and re-exports forbidden by module path or imported name. |  |  | 99.6% |
| [no-restricted-properties](no-restricted-properties.md) | Reports configured object properties, whether read, called or destructured. |  |  | 100% |
| [no-restricted-syntax](no-restricted-syntax.md) | Reports syntax added to the open `restricted_syntax` group. |  |  | 97.1% |
| [no-restricted-types](no-restricted-types.md) | Reports types forbidden by your configuration. |  | autofix | 100% |
| [no-return-assign](no-return-assign.md) | Reports assignments inside returned expressions. |  |  | 100% |
| [no-script-url](no-script-url.md) | Reports `javascript:` URLs in strings and template literals. |  |  | 100% |
| [no-self-assign](no-self-assign.md) | Reports assignments that copy a value back onto itself. |  |  | 100% |
| [no-self-compare](no-self-compare.md) | Reports comparisons with the same expression on both sides. |  |  | 100% |
| [no-sequences](no-sequences.md) | Reports comma operators that hide multiple operations in one expression. |  |  | 100% |
| [no-setter-return](no-setter-return.md) | Reports values returned from setters, where the caller cannot use them. |  |  | 100% |
| [no-shadow-restricted-names](no-shadow-restricted-names.md) | Reports bindings that replace restricted names such as `NaN`, `Infinity`, `undefined`, `eval` and `arguments`. |  |  | 100% |
| [no-shadow](no-shadow.md) | Reports declarations that hide names from an outer scope. |  |  | 100% |
| [no-sparse-arrays](no-sparse-arrays.md) | Reports holes in array literals, often caused by an accidental extra comma. |  |  | 100% |
| [no-sync](no-sync.md) | Reports calls to `Sync`-suffixed methods inside functions. |  |  |  |
| [no-tabs](no-tabs.md) | Reports tab characters anywhere in a file, including comments and strings. |  |  | 100% |
| [no-template-curly-in-string](no-template-curly-in-string.md) | Reports `${...}` placeholders inside ordinary quoted strings. |  |  | 100% |
| [no-ternary](no-ternary.md) | Reports conditional expressions using `? |  |  | 100% |
| [no-this-alias](no-this-alias.md) | Reports variables used to alias `this`. |  |  | 100% |
| [no-this-before-super](no-this-before-super.md) | Reports use of `this` or a `super` property before `super()` in a derived constructor. |  |  | 100% |
| [no-throw-literal](no-throw-literal.md) | Reports thrown expressions that cannot be error objects. |  |  | 100% |
| [no-trailing-spaces](no-trailing-spaces.md) | Reports whitespace at the end of a line. |  | autofix | 100% |
| [no-types](no-types.md) | Reports JSDoc tags with braced type annotations. |  |  |  |
| [no-unassigned-import](no-unassigned-import.md) | Reports imports without imported bindings. |  |  |  |
| [no-unassigned-vars](no-unassigned-vars.md) | Reports variables that are read but never assigned a value. |  |  | 100% |
| [no-undef-init](no-undef-init.md) | Reports `let` and `var` declarations explicitly initialised to `undefined`. |  | autofix | 100% |
| [no-undef](no-undef.md) | Reports references to names that have not been declared. |  |  | 100% |
| [no-undefined](no-undefined.md) | Reports use of the identifier `undefined`. |  |  | 100% |
| [no-underscore-dangle](no-underscore-dangle.md) | Reports names with leading or trailing underscores. |  |  | 100% |
| [no-unexpected-multiline](no-unexpected-multiline.md) | Reports confusing line breaks that continue an expression instead of starting a new statement. |  |  | 100% |
| [no-unmodified-loop-condition](no-unmodified-loop-condition.md) | Reports variables used in a loop condition but never modified in the loop. |  |  | 100% |
| [no-unnecessary-boolean-literal-compare](no-unnecessary-boolean-literal-compare.md) | Reports equality comparisons between booleans and `true` or `false`. | yes | autofix | 97.2% |
| [no-unnecessary-condition](no-unnecessary-condition.md) | Reports conditions whose types make the result fixed. | yes |  | 96.9% |
| [no-unnecessary-parameter-property-assignment](no-unnecessary-parameter-property-assignment.md) | Reports assignments that repeat a constructor parameter property's automatic assignment. |  |  | 100% |
| [no-unnecessary-qualifier](no-unnecessary-qualifier.md) | Reports enum and namespace qualifiers when the member is already in scope. | yes | autofix | 100% |
| [no-unnecessary-template-expression](no-unnecessary-template-expression.md) | Reports template substitutions that can be incorporated into the surrounding text, or replaced with the string expression itself. | yes | autofix | 100% |
| [no-unnecessary-type-arguments](no-unnecessary-type-arguments.md) | Reports explicit type arguments equal to the declared default. | yes | autofix | 100% |
| [no-unnecessary-type-assertion](no-unnecessary-type-assertion.md) | Reports assertions that do not change an expression's type. | yes | autofix | 93.9% |
| [no-unnecessary-type-constraint](no-unnecessary-type-constraint.md) | Reports generic type parameters constrained to `any` or `unknown`. |  |  | 100% |
| [no-unnecessary-type-conversion](no-unnecessary-type-conversion.md) | Reports conversion idioms that do not change a value's type or value. | yes |  | 100% |
| [no-unnecessary-type-parameters](no-unnecessary-type-parameters.md) | Reports type parameters that do not relate multiple parts of a signature. | yes |  | 100% |
| [no-unneeded-ternary](no-unneeded-ternary.md) | Reports ternaries that can be replaced with simpler boolean expressions. |  | autofix | 100% |
| [no-unreachable-loop](no-unreachable-loop.md) | Reports loops whose bodies always exit before a second iteration. |  |  | 100% |
| [no-unreachable](no-unreachable.md) | Reports code that cannot run after an unconditional exit. |  |  | 100% |
| [no-unsafe-argument](no-unsafe-argument.md) | Reports `any` values passed to typed parameters. | yes |  | 80.0% |
| [no-unsafe-assignment](no-unsafe-assignment.md) | Reports assignments of `any` to variables and properties. | yes |  | 100% |
| [no-unsafe-call](no-unsafe-call.md) | Reports calls through values typed as `any` or the unsafe `Function` type. | yes |  | 97.4% |
| [no-unsafe-declaration-merging](no-unsafe-declaration-merging.md) | Reports merging a class and interface with the same name. |  |  | 100% |
| [no-unsafe-enum-assignment](no-unsafe-enum-assignment.md) | Reports values that do not share the enum type expected by an assignment, argument, return, or other receiving location. | yes |  | 88.0% |
| [no-unsafe-enum-comparison](no-unsafe-enum-comparison.md) | Reports comparisons between enum values and values without a shared enum type, including switch cases. | yes |  | 100% |
| [no-unsafe-finally](no-unsafe-finally.md) | Reports `return`, `throw`, `break`, and `continue` statements that leave a `finally` block. |  |  | 100% |
| [no-unsafe-function-type](no-unsafe-function-type.md) | Reports the built-in `Function` type, which accepts functions without checking their parameters or return values. |  |  | 100% |
| [no-unsafe-member-access](no-unsafe-member-access.md) | Reports property access on `any` values and computed keys typed as `any`. | yes |  | 97.2% |
| [no-unsafe-negation](no-unsafe-negation.md) | Reports a negation applied directly to the left operand of `in` or `instanceof`. |  |  | 100% |
| [no-unsafe-optional-chaining](no-unsafe-optional-chaining.md) | Reports optional chains used where an `undefined` result would cause an error. |  |  | 100% |
| [no-unsafe-return](no-unsafe-return.md) | Reports unsafe `any` return values, including arrays and promises. | yes |  | 98.4% |
| [no-unsafe-type-assertion](no-unsafe-type-assertion.md) | Reports type assertions that narrow a value's type without a runtime check, including unsafe assertions involving `any`. | yes |  | 94.1% |
| [no-unsafe-unary-minus](no-unsafe-unary-minus.md) | Reports unary `-` applied to values that are not numeric. | yes |  | 100% |
| [no-unused-expressions](no-unused-expressions.md) | Reports expression statements whose values are discarded without a useful effect. |  |  | 100% |
| [no-unused-imports](no-unused-imports.md) | Reports imported names that are never used. |  |  |  |
| [no-unused-labels](no-unused-labels.md) | Reports labels that no `break` or `continue` refers to. |  | autofix | 100% |
| [no-unused-private-class-members](no-unused-private-class-members.md) | Reports private fields and methods whose values are never read, and private accessors that are never accessed. |  |  | 100% |
| [no-unused-vars](no-unused-vars.md) | Reports bindings that are declared or assigned but never usefully read. |  | autofix | 99.0% |
| [no-use-before-define](no-use-before-define.md) | Reports references that appear before their declarations. |  |  | 99.8% |
| [no-useless-assignment](no-useless-assignment.md) | Reports assignments whose stored values are never read later. |  |  | 95.9% |
| [no-useless-backreference](no-useless-backreference.md) | Reports regular-expression backreferences that can only match the empty string. |  |  | 100% |
| [no-useless-call](no-useless-call.md) | Reports `.call()` and `.apply()` uses that can be replaced with a direct call without changing the intended `this` value. |  |  | 100% |
| [no-useless-catch](no-useless-catch.md) | Reports catch clauses that only rethrow the same error. |  |  | 100% |
| [no-useless-computed-key](no-useless-computed-key.md) | Reports literal property keys wrapped in unnecessary square brackets. |  | autofix | 100% |
| [no-useless-concat](no-useless-concat.md) | Reports adjacent string or template literals joined with `+` when one literal would do. |  |  | 100% |
| [no-useless-constructor](no-useless-constructor.md) | Reports constructors that add nothing to a class's default construction behaviour. |  |  | 100% |
| [no-useless-default-assignment](no-useless-default-assignment.md) | Reports default values that cannot be used because the incoming value cannot be `undefined`, and defaults that merely supply `undefined`. | yes |  | 100% |
| [no-useless-empty-export](no-useless-empty-export.md) | Reports empty exports in files already made into modules by another import or export. |  | autofix | 100% |
| [no-useless-escape](no-useless-escape.md) | Reports escapes that do not change a string, template literal, or regular expression's meaning. |  |  | 100% |
| [no-useless-rename](no-useless-rename.md) | Reports imports, exports, and destructuring bindings that rename a name to itself. |  | autofix | 100% |
| [no-useless-return](no-useless-return.md) | Reports bare `return;` statements that do not change a function's control flow. |  | autofix | 100% |
| [no-var-requires](no-var-requires.md) | Reports `require()` calls used to obtain a module value outside an import declaration. |  |  |  |
| [no-var](no-var.md) | Reports `var` declarations. |  | autofix | 100% |
| [no-void](no-void.md) | Reports the `void` operator, which evaluates an expression and discards its result. |  |  | 100% |
| [no-warning-comments](no-warning-comments.md) | Reports comments beginning with warning terms such as `TODO` or `FIXME`. |  |  | 96.7% |
| [no-whitespace-before-property](no-whitespace-before-property.md) | Reports spaces around a property-access dot or before a computed-property bracket on the same line. |  | autofix | 100% |
| [no-with](no-with.md) | Reports `with` statements. |  |  | 100% |
| [no-wrapper-object-types](no-wrapper-object-types.md) | Reports global wrapper types such as `String`, `Number` and `Boolean`. |  | autofix | 100% |
| [non-nullable-type-assertion-style](non-nullable-type-assertion-style.md) | Reports type assertions that only remove `null` and `undefined`. | yes | autofix | 84.6% |
| [nonblock-statement-body-position](nonblock-statement-body-position.md) | Controls whether a single-statement body without braces sits beside or below its condition. |  | autofix | 100% |
| [object-curly-spacing](object-curly-spacing.md) | Controls spaces inside object-like braces, including destructuring, named imports and TypeScript type bodies. |  | autofix |  |
| [object-shorthand](object-shorthand.md) | Controls shorthand properties and methods in object literals. |  | autofix | 100% |
| [one-var-declaration-per-line](one-var-declaration-per-line.md) | Reports variable declarators that should start on separate lines. |  | autofix | 100% |
| [one-var](one-var.md) | Controls whether variable declarations are combined or separated within a scope. |  | autofix | 100% |
| [only-throw-error](only-throw-error.md) | Reports thrown values whose types are not `Error` objects or allowed exceptions. | yes |  | 97.8% |
| [operator-assignment](operator-assignment.md) | Requires or forbids compound arithmetic and bitwise assignments such as `+=`. |  | autofix | 100% |
| [operator-linebreak](operator-linebreak.md) | Controls which side of an operator a line break appears on. |  | autofix | 100% |
| [padded-blocks](padded-blocks.md) | Controls blank lines just inside blocks, classes and switches. |  | autofix | 100% |
| [padding-line-between-statements](padding-line-between-statements.md) | Requires or forbids blank lines between selected kinds of statements. |  | autofix | 100% |
| [parameter-properties](parameter-properties.md) | Controls TypeScript constructor parameter properties. |  |  | 100% |
| [prefer-arrow-callback](prefer-arrow-callback.md) | Reports function expressions used as callbacks when an arrow function can replace them without changing their meaning. |  | autofix | 100% |
| [prefer-as-const](prefer-as-const.md) | Reports repeated literal types that can be expressed with a const assertion. |  | autofix | 100% |
| [prefer-const](prefer-const.md) | Reports `let` bindings that are assigned once and never reassigned. |  | autofix | 97.8% |
| [prefer-destructuring](prefer-destructuring.md) | Reports property or array-element reads that can be written as destructuring declarations or assignments. | yes | autofix | 100% |
| [prefer-enum-initializers](prefer-enum-initializers.md) | Reports enum members without explicit values. |  |  | 100% |
| [prefer-exponentiation-operator](prefer-exponentiation-operator.md) | Reports calls to `Math.pow` that can use the `**` operator. |  | autofix | 100% |
| [prefer-find](prefer-find.md) | Reports filtering an array just to take its first matching element. | yes |  | 100% |
| [prefer-for-of](prefer-for-of.md) | Reports index-based loops whose index is used only to read the current element. |  |  | 97.1% |
| [prefer-function-type](prefer-function-type.md) | Reports interfaces and object type literals containing only one call signature when a function type can express the same contract. |  | autofix | 100% |
| [prefer-includes](prefer-includes.md) | Reports membership checks written as `indexOf` comparisons or simple regular expression tests. | yes | autofix | 97.6% |
| [prefer-literal-enum-member](prefer-literal-enum-member.md) | Requires explicit enum values to be string or number literals. |  |  | 100% |
| [prefer-named-capture-group](prefer-named-capture-group.md) | Reports unnamed capturing groups in regular expressions. |  |  | 100% |
| [prefer-namespace-keyword](prefer-namespace-keyword.md) | Requires `namespace` rather than the older `module` keyword for TypeScript namespaces. |  | autofix | 100% |
| [prefer-nullish-coalescing](prefer-nullish-coalescing.md) | Reports default-value expressions that can use `??` or `??=`. | yes |  | 100% |
| [prefer-numeric-literals](prefer-numeric-literals.md) | Reports `parseInt` calls that convert a fixed binary, octal, or hexadecimal string. |  | autofix | 100% |
| [prefer-object-has-own](prefer-object-has-own.md) | Reports indirect `hasOwnProperty` calls that can use `Object.hasOwn`. |  | autofix | 100% |
| [prefer-object-spread](prefer-object-spread.md) | Reports `Object.assign` calls whose first argument is an object literal. |  | autofix | 99.1% |
| [prefer-optional-chain](prefer-optional-chain.md) | Reports repeated nullish guards that can use optional chaining. | yes | autofix | 98.8% |
| [prefer-promise-reject-errors](prefer-promise-reject-errors.md) | Requires Promise rejection reasons to be errors. | yes |  | 100% |
| [prefer-readonly-parameter-types](prefer-readonly-parameter-types.md) | Requires parameter types to be deeply readonly. | yes |  | 99.2% |
| [prefer-readonly](prefer-readonly.md) | Requires private members to be `readonly` when they are not reassigned outside the constructor. | yes | autofix | 100% |
| [prefer-reduce-type-parameter](prefer-reduce-type-parameter.md) | Reports assertions on the initial value passed to `Array#reduce`. | yes | autofix | 100% |
| [prefer-regex-literals](prefer-regex-literals.md) | Reports `RegExp` constructors with fixed patterns and flags. |  |  | 100% |
| [prefer-regexp-exec](prefer-regexp-exec.md) | Prefers `RegExp#exec` to `String#match` when the expression has no global flag. | yes | autofix | 100% |
| [prefer-rest-params](prefer-rest-params.md) | Reports use of the implicit `arguments` object. |  |  | 100% |
| [prefer-return-this-type](prefer-return-this-type.md) | Prefers the `this` return type when a class member returns its own instance. | yes | autofix | 100% |
| [prefer-spread](prefer-spread.md) | Reports `.apply()` calls that can use argument spread without changing the receiver. |  |  | 100% |
| [prefer-string-starts-ends-with](prefer-string-starts-ends-with.md) | Reports string boundary checks that can use `startsWith` or `endsWith`. | yes | autofix | 100% |
| [prefer-template](prefer-template.md) | Reports string concatenation with non-literal values. |  | autofix | 100% |
| [preserve-caught-error](preserve-caught-error.md) | Requires new errors thrown inside a catch block to preserve the caught error as their `cause`. |  |  | 100% |
| [promise-function-async](promise-function-async.md) | Requires Promise-returning functions to be `async`. | yes | autofix | 100% |
| [quote-props](quote-props.md) | Enforces a consistent policy for quoting property names. |  | autofix | 100% |
| [quotes](quotes.md) | Enforces one string delimiter style: double quotes by default. |  | autofix | 100% |
| [radix](radix.md) | Requires an explicit radix for `parseInt` and rejects invalid radix values. |  |  | 100% |
| [related-getter-setter-pairs](related-getter-setter-pairs.md) | Requires a getter's return type to be assignable to the corresponding setter's parameter type. | yes |  | 100% |
| [require-array-sort-compare](require-array-sort-compare.md) | Requires a comparison function for array `sort` and `toSorted` calls. | yes |  | 100% |
| [require-atomic-updates](require-atomic-updates.md) | Reports updates that read a shared value before `await` or `yield` and write it afterwards. |  |  | 100% |
| [require-await](require-await.md) | Reports async functions that neither await work nor return promises. | yes |  | 100% |
| [require-unicode-regexp](require-unicode-regexp.md) | Requires regular expressions to use Unicode mode with a `u` or `v` flag. |  |  | 100% |
| [require-yield](require-yield.md) | Reports nonempty generator functions that never yield a value. |  |  | 100% |
| [rest-spread-spacing](rest-spread-spacing.md) | Checks spacing between `...` and a rest or spread expression. |  | autofix | 100% |
| [restrict-plus-operands](restrict-plus-operands.md) | Checks the types used with `+` and `+=` to catch accidental coercion and incompatible addition. | yes |  | 100% |
| [restrict-template-expressions](restrict-template-expressions.md) | Checks values interpolated into template strings to avoid surprising text such as `[object Object]`. | yes |  | 100% |
| [return-await](return-await.md) | Checks whether promises returned from async functions should be awaited. | yes | autofix | 100% |
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
| [semi-spacing](semi-spacing.md) | Controls spaces before and after semicolons. |  | autofix | 100% |
| [semi-style](semi-style.md) | Controls which line holds a semicolon separating statements. |  | autofix | 100% |
| [semi](semi.md) | Enforces consistent statement-ending semicolons. |  | autofix | 99.7% |
| [sort-imports](sort-imports.md) | Checks import declaration order and alphabetical order of named import members. |  | autofix (members only) | 100% |
| [sort-keys](sort-keys.md) | Requires object keys in alphabetical order. |  |  | 100% |
| [sort-vars](sort-vars.md) | Requires variables within one declaration to be in alphabetical order. |  | autofix | 100% |
| [space-before-blocks](space-before-blocks.md) | Requires or disallows spaces before block-opening braces. |  | autofix | 100% |
| [space-before-function-paren](space-before-function-paren.md) | Controls spacing before function parameter lists. |  | autofix | 99.1% |
| [space-in-parens](space-in-parens.md) | Controls spaces immediately inside parentheses. |  | autofix | 100% |
| [space-infix-ops](space-infix-ops.md) | Requires spaces around infix operators, assignments and conditional-expression punctuation. |  | autofix | 100% |
| [space-unary-ops](space-unary-ops.md) | Controls spaces beside unary operators. |  | autofix | 100% |
| [spaced-comment](spaced-comment.md) | Controls whitespace after line and block comment openers. |  | autofix | 99.0% |
| [strict-boolean-expressions](strict-boolean-expressions.md) | Reports unsafe types in boolean conditions, logical negations, assertion arguments and array predicates. | yes |  | 98.2% |
| [strict-void-return](strict-void-return.md) | Reports value-returning functions used where a void-returning callback is expected. | yes |  | 96.8% |
| [strict](strict.md) | Requires or disallows `"use strict"` directives. |  | autofix | 99.2% |
| [switch-colon-spacing](switch-colon-spacing.md) | Controls spaces around `case` and `default` colons when the adjacent tokens are on the same line. |  | autofix | 100% |
| [switch-exhaustiveness-check](switch-exhaustiveness-check.md) | Reports missing cases when switching over a union of literal types or an enum. | yes |  | 100% |
| [symbol-description](symbol-description.md) | Requires a description when creating a symbol. |  |  | 100% |
| [tab-indent](tab-indent.md) | Reports tabs in leading indentation and replaces each tab with a fixed number of spaces. |  | autofix |  |
| [template-curly-spacing](template-curly-spacing.md) | Controls spaces inside template substitutions. |  | autofix | 100% |
| [template-tag-spacing](template-tag-spacing.md) | Controls whitespace between a template tag and its template literal. |  | autofix (except comment-separated tags) | 100% |
| [trailing-whitespace](trailing-whitespace.md) | Removes spaces and tabs at line ends, including whitespace-only lines. |  | autofix |  |
| [triple-slash-reference](triple-slash-reference.md) | Controls TypeScript triple-slash references, preferring imports where possible. |  |  | 100% |
| [type-annotation-spacing](type-annotation-spacing.md) | Controls spaces around TypeScript type annotation punctuation. |  | autofix | 100% |
| [type-generic-spacing](type-generic-spacing.md) | Keeps spacing around TypeScript generic brackets consistent. |  | autofix | 100% |
| [type-named-tuple-spacing](type-named-tuple-spacing.md) | Requires named tuple members to use `name: Type` or `name?: Type`. |  | autofix | 100% |
| [unbound-method](unbound-method.md) | Reports methods extracted from their object without binding their receiver. | yes |  | 98.6% |
| [unicode-bom](unicode-bom.md) | Controls whether a file starts with the Unicode byte order mark, U+FEFF. |  | autofix | 100% |
| [unified-signatures](unified-signatures.md) | Reports overloads that could be combined using a union, an optional parameter or a rest parameter. |  |  | 100% |
| [use-isnan](use-isnan.md) | Reports comparisons with `NaN`, including `Number.NaN`. |  |  | 100% |
| [use-unknown-in-catch-callback-variable](use-unknown-in-catch-callback-variable.md) | Requires Promise rejection callback parameters to have type `unknown` rather than `any` or a narrower assumed error type. | yes |  | 100% |
| [valid-typeof](valid-typeof.md) | Reports comparisons between `typeof` results and invalid type names. |  |  | 100% |
| [vars-on-top](vars-on-top.md) | Requires `var` declarations to appear together at the start of their function, program or class static block. |  |  | 100% |
| [wrap-iife](wrap-iife.md) | Requires parentheses around immediately invoked function expressions. |  | autofix | 100% |
| [wrap-regex](wrap-regex.md) | Requires parentheses around regular expression literals used as the object of a property access. |  | autofix | 100% |
| [yield-star-spacing](yield-star-spacing.md) | Keeps spacing around the star in delegated `yield*` expressions consistent. |  | autofix | 100% |
| [yoda](yoda.md) | Controls which side of a comparison holds a literal value. |  | autofix | 100% |
