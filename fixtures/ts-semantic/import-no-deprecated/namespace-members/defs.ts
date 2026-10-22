/** @deprecated use fresh */
export function old(): number { return 1; }
export function fresh(): number { return 2; }

/** @deprecated use modern */
export namespace legacy {
	/** @deprecated use modernFresh */
	export function inner(): number { return 1; }
	export function fresh(): number { return 2; }
}

export namespace modern {
	export function fresh(): number { return 2; }
}
