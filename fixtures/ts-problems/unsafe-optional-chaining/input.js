(obj?.a)();
new (obj?.Ctor)();
(obj?.a).b;
(obj?.a)[key];
(obj?.tag)`text`;
const {a} = obj?.value;
const [a] = obj?.value;
({a} = obj?.value);
[a] = obj?.value;
function defaults({a} = obj?.value) {}
class C extends (obj?.Base) {}
for (const item of obj?.items) {}
[...obj?.items];
consume(...obj?.args);
key in obj?.value;
value instanceof obj?.Ctor;
with (obj?.scope) {}
(obj?.fn || fallback)();
(fallback || obj?.fn)();
(obj?.fn ?? fallback)();
(fallback ?? obj?.fn)();
(obj?.left && obj?.right)();
(obj?.ignored, fallback)();
(fallback, obj?.fn)();
(condition ? obj?.left : obj?.right)();
async function awaited() { (await obj?.fn)(); }
function* delegated() { yield* obj?.items; }
function* direct() { yield obj?.items; }
obj?.fn(); obj?.value.child; obj?.fn?.();
({ ...obj?.value }); obj?.value + 1; +obj?.value; a += obj?.value;
(obj?.fn)?.(); (obj?.value)?.child;
((obj?.value)).child;
(obj?.value.child)();
(obj?.fn()).child;
(obj?.flag ? left : right)();
[{a} = obj?.value] = values;
