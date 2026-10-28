export function f(x: number, y: boolean): number {
	let s = 0;
	switch (x) {
		case 1:
			if (y) {
				s = 1;
			}

		// fallthrough

		case 2:
			s += 2;
			break;
		case 3:
			s = 3;
		// fall through to the shared handling
		case 4:
			s = 4;
			// falls through
		case 5:
			s = 5;
		case 6:
			s = 6;
	}
	return s;
}
