function outer() { "use strict"; function inner() {  work(); } }
class C { method() {  work(); } }
function duplicates() { "use strict";  work(); }
function required() { work(); }
function wrap(value = 1) { work(); }
const arrow = () => { "use strict";  work(); };
function comment() { "use strict"; "use strict" /* keep */; work(); }
