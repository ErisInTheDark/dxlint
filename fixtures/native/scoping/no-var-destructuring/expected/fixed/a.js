let { a, b = a } = {};
let [first, second = first] = [];
let { key: renamed, ...rest } = {};
function early() { var { a = b, b = 1 } = {}; }
function self() { var { value = value } = {}; }
function duplicate() { var { x } = {}; var x = 1; }
function computed() { let { [key]: value } = {}; }
function captured() { for (var { x } of list) { save(() => x); } }
function caught() { try {} catch (x) { var { x } = {}; } }
