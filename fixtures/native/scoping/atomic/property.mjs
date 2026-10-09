async function f(obj) { if (obj.ready) { await work(); obj.ready = false; } }
