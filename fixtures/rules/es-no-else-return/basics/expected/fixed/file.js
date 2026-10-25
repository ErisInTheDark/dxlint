// Unicode prefix: é
function blocks() { if (test) { return 1; }  work("é");  }
function statements() { if (test) return 1; work(); }
function comments() { if (test) { return 1; } /* stay */  /* body */ work(); /* tail */  }
function chain() { if (one) { return 1; } else if (two) { return 2; }  work();  }
function multiline() { if (test) return value
 work(); 
[1, 2].map(work); }
