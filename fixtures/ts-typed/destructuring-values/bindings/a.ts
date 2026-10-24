interface Item {
	id: number;
	tag: string;
}

declare const items: Item[];
declare const pair: [number, string];
declare const optional: { id: number; label?: string };
declare const nested: { outer: { inner: string } };
declare const pairs: [string, number][];

const [first, second] = items;
first;
second;

const [count, name] = pair;
count;
name;

const [head, ...rest] = items;
head;
rest;

const { id, label: renamed } = optional;
id;
renamed;

const { label = "none" } = optional;
label;

const { outer: { inner } } = nested;
inner;

for (const item of items) {
	item.id;
}

for (const { id: loopId, tag: loopTag } of items) {
	loopId;
	loopTag;
}

for (const [key, value] of pairs) {
	key;
	value;
}

function describe({ id: describedId, tag }: Item): string {
	describedId;
	tag;
	return tag;
}
