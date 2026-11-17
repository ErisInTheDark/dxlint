# es-wrap-multiline-expression

Requires parentheses, closed on their own line, around a multiline expression that something follows on its last line.

A multiline expression has operands on different lines: a chain whose links start new lines, `&&`/`+`/… with a line break around the operator, a conditional broken before `?` or `:`. What follows is a `,`, `;`, `)`, `]` or `}`. The fix wraps it in parentheses with `)` on its own line, so adding a part never touches the line carrying that punctuation. Line breaks only inside brackets (arguments, object literals, function bodies) do not count, an expression that is a whole statement is left alone, and only the outermost expression reports. The fix never moves operators; `es-operator-linebreak` places them.

| | |
| --- | --- |
| Upstream | none |
| Needs types | no |
| Fix | autofix |
| Rulesets | none |
| Parity | not measured |

## Enable

```dx
import .ts
rule es-wrap-multiline-expression
```

## Examples

Incorrect:

```ts
builder.add("fill", values => values
	.map(value => value * 2)
	.filter(value => value > 1));
const ready = loaded
	&& !failed
	&& queue.length === 0;
```

Correct:

```ts
builder.add("fill", values => (values
	.map(value => value * 2)
	.filter(value => value > 1)
));
const ready = (loaded
	&& !failed
	&& queue.length === 0
);
// nothing follows on the last line
const loose = list
	.map(value => value)
list
	.map(value => value)
	.forEach(log);
```

`es-no-extra-parens` leaves these parentheses alone.
