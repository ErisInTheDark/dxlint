interface Section { text: string }
type Arg = string | number | boolean | Section | { [key: string]: Arg } | (() => Arg) | undefined | null;

// No primitive is typeof "object": the strings are gone here.
export function walk(argument: Arg): void {
	if (typeof argument !== "object" || argument === null) return;
	for (const key in argument) { void key; }
}

// Arrays still are.
export function arrays(list: number[] | string): void {
	if (typeof list !== "object") return;
	for (const key in list) { void key; }
}
