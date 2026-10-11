declare function v(): void;
declare function consume(value: unknown): void;
void v();
const arrow = () => void v();
function returned() { return void v(); }
consume(v());
