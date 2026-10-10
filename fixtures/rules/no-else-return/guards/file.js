function localCollision() { let a; if (test) return 1; else { let a; } }
function parameter(a) { if (test) return 1; else { let a; } }
function outerReference() { let a; if (one) { work(a); if (two) return 1; else { let a; } } }
function catchCollision() { try {} catch (a) { if (test) return 1; else { let a; } } }
function argsUsed() { work(arguments); if (test) return 1; else { let arguments; } }
function varCollision() { if (test) return 1; else { let a; } if (other) { var a; } }
function commentGap() { if (test) return 1; else /* keep */ { work(); } }
function beforeASI() { if (test) return value
else { [1, 2].map(work); } }
function afterASI() { if (test) return value; else { work() }
[1, 2].map(work); }
function escaped() { let \u0061; if (test) return 1; else { let a; } }
