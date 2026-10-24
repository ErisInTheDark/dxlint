interface Item {
	id: number;
	tag: string;
}

declare const grid: Item[][];
declare const maybe: { id?: number; tag: string };
declare const values: { nested: { deep: number } };
declare const source: Item;

const [fromGrid] = grid[0];
fromGrid;

const { nested: { deep } } = values;
deep;

const { id: optionalId = 4 } = maybe;
optionalId;

const [one = 1, two = "x"] = [] as [number?, string?];
one;
two;

let [widened] = [1];
widened;

const [literal] = [1] as const;
literal;

const { id: fromSource } = source;
fromSource;
