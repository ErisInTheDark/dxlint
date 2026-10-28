export function watch(output: string, resolve: () => void): void {
	let done = false;
	if (output.includes("finished")) {done = true;}
	if (done) {resolve();}
	const value = { toString() {
		return "x";
	} };
	console.log(value);
}
