function safe() {}
function unsafe() {}
const object = { okay() {}, blocked() {} };
safe();
unsafe();
object.okay();
object.blocked();
const alias = safe;
alias();
