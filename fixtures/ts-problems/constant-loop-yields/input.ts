function* plain() { while (1) {} }
function* bodyYield() { while (1) { yield 0; } }
function* nestedFunction() { while (1) { function* inner() { yield 0; } } }
function* classKey() { while (1) { class C { [yield 0]() {} } } }
function* classHeritage() { while (1) { class C extends (yield 0) {} } }
function* initialOnly() { for (yield 0; 1;) {} }
function* outerInitial() { while (1) { for (yield 0; x;) {} } }
function* deadYield() { while (1) { if (false) { yield 0; } } }
function* whileTrue() { while (true) {} }
function* doYield() { do { yield 0; } while (false); }
function* updateYield() { for (; 1; yield 0) {} }
function* methodBody() { while (1) { class C { *inner() { yield 0; } } } }
function* keyField() { while (1) { class C { [yield 0] = 1; } } }
function* noYield() { do {} while (false); }
