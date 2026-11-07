let a = 0;
let b = 0;
declare function run(): void;
declare const ready: boolean;

if (ready)
	a = 1, b = 2;
run(), a++, (b = 3, run());
a, run();
run(), a + b;
ready && run(), a = 4;
