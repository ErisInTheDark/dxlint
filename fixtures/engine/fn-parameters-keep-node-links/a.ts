declare class Box { constructor(value: number); }

export function make(): Box {
	const kept = 1;
	const passed = 2;
	void kept;
	return new Box(passed);
}
