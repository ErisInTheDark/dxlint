function branch() { if (test) { run(); return; } else { other(); } }
function cases() { switch (value) { case 1: run(); break; default: return; } }
function caught() { try {} catch (err) { return; } }
function tried() { try { run(); return; } finally { cleanup(); } }
function nestedFinally() { try { return 5; } finally { function inner() { return; } } }
