const assign = Object.assign;
const a = { ...foo};
const b = { ...foo};
function shadow(Object) { return Object.assign({}, foo); }
