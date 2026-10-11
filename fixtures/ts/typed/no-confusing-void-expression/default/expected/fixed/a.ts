declare function v(): void;
declare function u(): undefined;
declare function mixed(): void | number;
declare function consume(value: unknown): void;
declare const test: boolean;
v();
test && v();
test ? v() : u();
(v(), 1);
(1, v());
consume(v());
v() && test;
v() ? 1 : 2;
const a = () => { v(); };
function last() { v(); }
function early() { if (test) { v(); return; } consume(1); }
const union = mixed();
const undef = u();
const mixedArrow = () => test ? 1 : v();
function mixedReturn() { return test ? 1 : v(); }
function commented() { return /* keep */ v(); }
function asi() {
  if (test) { consume(1) }
  ;(v());
}
declare function promise(): Promise<void>;
async function awaiting() { const value = await promise(); }
declare function tag(parts: TemplateStringsArray): void;
const tagged = tag`x`;
interface Promise<T> { then(callback: (value: T) => unknown): unknown }
const object = { method() { v(); } };
const grouped = () => { test ? v() : v(); };
