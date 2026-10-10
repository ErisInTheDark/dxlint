function branch() { if (test) { run();  } else { other(); } }
function cases() { switch (value) { case 1: run(); break; default:  } }
function caught() { try {} catch (err) {  } }
function tried() { try { run();  } finally { cleanup(); } }
function nestedFinally() { try { return 5; } finally { function inner() {  } } }
