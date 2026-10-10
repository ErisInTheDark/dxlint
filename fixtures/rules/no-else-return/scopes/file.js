function lexical() { if (test) { return 1; } else { let local = 2; work(local); } }
function outerUnused() { let a; if (test) { if (inner) { return 1; } else { let a; } } }
function catchUnused() { try {} catch (a) { if (test) { if (inner) { return 1; } else { let a; } } } }
function argsUnused() { if (test) { return 1; } else { let arguments; } }
function varOutside() { if (test) { if (inner) { return 1; } else { let a; } } if (other) { var a; } }
