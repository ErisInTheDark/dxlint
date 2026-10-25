// Unicode prefix: é
function blocks() { if (test) { return 1; } else { work("é"); } }
function statements() { if (test) return 1; else work(); }
function comments() { if (test) { return 1; } /* stay */ else { /* body */ work(); /* tail */ } }
function chain() { if (one) { return 1; } else if (two) { return 2; } else { work(); } }
function multiline() { if (test) return value
else { work(); }
[1, 2].map(work); }
