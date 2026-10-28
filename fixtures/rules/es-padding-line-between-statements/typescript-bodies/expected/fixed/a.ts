export namespace Repro {
	export function a(): number {
		return 1;
	};

	export function b(): number {
		return 2;
	}
	export const c = 3;
}

function d(): void {
	if (Repro.c) {
		Repro.a();
	}

	Repro.b();
}

d();
