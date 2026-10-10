function outer() { "use strict"; function inner() { "use strict"; work(); } }
class C { method() { "use strict"; work(); } }
function duplicates() { "use strict"; 'use strict'; work(); }
function required() { work(); }
function wrap(value = 1) { work(); }
const arrow = () => { "use strict"; "use strict"; work(); };
function comment() { "use strict"; "use strict" /* keep */; work(); }
