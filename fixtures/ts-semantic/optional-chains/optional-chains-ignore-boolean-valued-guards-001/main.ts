interface Description {
	decayMax?: number;
	isFlammable?: boolean;
	use?: Use;
}

interface Use {
	includes(value: string): boolean;
}

interface Crafted {
	newUnlock?: boolean;
}

declare const description: Description;
declare const stage: number;
declare const entry: Crafted | undefined;

declare function log(value: unknown): void;

// An optional boolean in the middle of a chain narrows to `false`, not nullish.
if (description && description.isFlammable && stage !== 3) log(stage);

// A guard that is only ever a boolean (here a call result) is not a nullish check.
if (
	description.decayMax !== undefined
	&& description.use !== undefined
	&& description.use.includes("Eat")
	&& !description.use.includes("Preserve")
) log(stage);

// `!x && y` carries `false`, so the chain would change type.
const crafted = !description.isFlammable && stage !== 3 && entry;
if (crafted && crafted.newUnlock) log(stage);

export {};
