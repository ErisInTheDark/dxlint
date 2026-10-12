var { a, b = a } = {};
var [first, second = first] = [];
var { key: renamed, ...rest } = {};
function early() { var { a = b, b = 1 } = {}; }
function self() { var { value = value } = {}; }
function duplicate() { var { x } = {}; var x = 1; }
function computed() { var { [key]: value } = {}; }
function captured() { for (var { x } of list) { save(() => x); } }
function caught() { try {} catch (x) { var { x } = {}; } }
