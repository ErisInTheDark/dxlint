function over(x: string): string;
function over(x: number): number;
function over(x: any): any { return x; }
function plain() { return 1; }
function loop() { return loop(); }
over(1);
over("a");
plain();
loop();
