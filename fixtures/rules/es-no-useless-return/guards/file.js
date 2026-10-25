function unbraced() { if (test) return; }
function internal() { return /* keep */; }
function valued() { return value; }
function loop() { while (test) { return; } }
function finalizer() { try { work(); } finally { return; } }
function useful() { if (test) { return; } work(); }
function unreachable() { throw error; return; }
