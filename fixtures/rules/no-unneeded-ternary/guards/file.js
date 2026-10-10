const a = foo() ? true : true;
const b = x /*keep*/ ? true : false;
const c = x ? /*keep*/ false : true;
const d = x ? x /*keep*/ : y;
const e = foo() ? true : true /*outside*/;
