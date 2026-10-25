const a = x || foo();
const b = ((x)) || ((y));
const c = x || (y ? z : w);
const d = x || (y ?? z);
const e = x || (y => y);
const f = x || (y = 0);
const g = x || (y, z);
function* h() { return x || (yield y); }
const i = x || (!!y);
