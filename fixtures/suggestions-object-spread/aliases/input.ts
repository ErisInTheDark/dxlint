const assign = Object.assign;
const a = assign({}, foo);
const b = globalThis.Object.assign({}, foo);
function shadow(Object) { return Object.assign({}, foo); }
