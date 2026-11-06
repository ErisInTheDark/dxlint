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
