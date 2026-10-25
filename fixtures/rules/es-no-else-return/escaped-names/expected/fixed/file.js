function safe() { if (test) return 1;  let \u0061 = 2; work(a);  }
function collision() { let \u0061; if (test) return 1; else { let a; } }
function captured() { work(\u0061); if (test) return 1; else { let a; } }
function unrelated() { let \u0062; if (test) return 1;  let a; work(a);  }
