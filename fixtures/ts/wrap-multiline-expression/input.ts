declare const list: number[];
declare const loaded: boolean, failed: boolean, ok: boolean;
declare function take(...values: unknown[]): void;
declare const builder: { add(name: string, make: (x: number[]) => unknown): typeof builder };

// reported: a comma, semicolon or bracket follows on the last line
builder
	.add("fill", values => values
		.map(value => value * 2)
		.filter(value => value > 1))
	.add("plain", values => values);
const doubled = list
	.map(value => value * 2)
	.filter(value => value > 1);
const ready = loaded
	&& !failed
	&& list.length === 0;
let picked;
picked = ok
	? list
	: [];
take(list
	.map(value => value), 1);
const items = [list
	.map(value => value), list];
const record = { values: list
	.map(value => value) };
function first() {
	return list
		.map(value => value)
		.at(0);
}
const mixed = loaded
	&& list
		.map(value => value)
		.includes(1);

// silent
list
	.map(value => value)
	.forEach(value => take(value));
const single = list.map(value => value).filter(Boolean);
const wrapped = (list
	.map(value => value)
);
const inside = take(list.map(value => {
	return value;
})) ?? list.length;
take(1, list
	.map(value => value)
);
const loose = list
	.map(value => value)
take(loose);
