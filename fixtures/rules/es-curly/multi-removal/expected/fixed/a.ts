export function pick(values: number[]): number | undefined {
	for (const value of values) {
		if (value > 1)
			continue;
		if (value < 0)
			return value;
		else
			return 0;
	}
	if (values.length) return values[0];
	return undefined;
}
