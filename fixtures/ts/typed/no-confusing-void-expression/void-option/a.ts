declare function v(): void;
declare function consume(value: unknown): void;
void v();
const arrow = () => v();
function returned() { return v(); }
consume(v());
