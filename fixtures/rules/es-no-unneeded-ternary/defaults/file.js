const a = x ? x : foo();
const b = ((x)) ? (((x))) : ((y));
const c = x ? x : y ? z : w;
const d = x ? x : y ?? z;
const e = x ? x : y => y;
const f = x ? x : y = 0;
const g = x ? x : (y, z);
function* h() { return x ? x : yield y; }
const i = x ? x : y ? true : false;
