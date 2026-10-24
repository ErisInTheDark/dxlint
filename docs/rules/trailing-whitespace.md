# trailing-whitespace

Removes spaces and tabs at line ends, including whitespace-only lines. This is a text-level rule for every declared language.

| | |
| --- | --- |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |

## Enable

```dx
import .ts
rule trailing-whitespace
```

Any language import brings in the shared whitespace rules.

## Examples

The incorrect line has spaces after its semicolon.

Incorrect:

```ts
run();  
```

Correct:

```ts
run();
```

The rule checks raw text, including line endings inside comments and string literals. It has no options.
