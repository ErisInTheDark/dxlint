declare const a: boolean, b: boolean, c: boolean;

export function run(): void {
	if (a && b) {
		void 0;
	} else {
		void 1;
	}

	if (a) {
		void 2;
	} else if (c) {
		void 3;
	}

	if (a < b) {
		void 4;
	}
}
