declare function open(): Disposable;
declare function openAsync(): AsyncDisposable;
declare function both(): Disposable & AsyncDisposable;
declare function plain(): { close(): void };
declare const uncertain: any;
class Resource implements Disposable {
  [Symbol.dispose]() {}
}

const stored = open();
let storedAsync = openAsync();
var storedBoth = both();
const alias = stored;
const fresh = new Resource();
const literal = { [Symbol.dispose]() {} };
const named = { dispose() {} };
const primitive = 1 + 1;
const notDisposable = plain();
const dynamic = uncertain;

open();
openAsync();
(open());
new Resource();
void open();
plain();
uncertain;
let target: Disposable;
target = open();

using held = open();
await using awaited = openAsync();

interface Walker extends Disposable { next(): void }
declare function walk(): Walker;
declare function pair(): Disposable & [number];
declare const box: { held: Disposable };
const walker = walk();
const [element] = pair();
function returned() { const owned = open(); return owned; }
const moved = open();
box.held = moved;
const wrapped = open();
const shape = { wrapped };
class Owner { constructor(held: Disposable) {} }
const given = open();
new Owner(given);
walk();
declare function consume(held: Disposable): void;
function parenthesized() { const a = open(); return (a); }
function conditional(flag: boolean) { const a = open(); return flag ? a : null; }
function logical(flag: boolean) { const a = open(); return flag && a; }
function cast() { const a = open(); return a as Disposable; }
function nonNull() { const a = open(); return a!; }
function array() { const a = open(); return [a]; }
const concise = () => { const a = open(); return () => a; };
function* yielded() { const a = open(); yield a; }
function aliased() { const a = open(); const b = a; return b; }
function assigned() { let a; a = open(); return a; }
function member(this: { held: Disposable }) { this.held = open(); }
function testedOnly() { const a = open(); return a ? 1 : 2; }
function borrowed() { const a = open(); consume(a); }
function assignedLocally() { let a; a = open(); consume(a); }
function cycle() { let a = open(); let b = a; a = b; b = a; }
