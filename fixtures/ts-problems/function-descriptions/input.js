function plain() {}
function \u0066oo() {}
async function asynchronous() {}
function* generator() {}
const anonymous = function() {};
const arrow = () => {};
const object = {
  field: function internal() {},
  grouped: (function internal() {}),
  arrow: () => {},
  async method() {},
  *generator() {},
  get value() { return 1; },
  set value(v) {},
  ["fixed"]: function() {},
  [dynamic]: function own() {},
};
class Holder {
  constructor() {}
  static constructor() {}
  get value() { return 1; }
  static get constructor() { return 1; }
  #\u0078() {}
  static async *method() {}
  field = (function own() {});
  #secret = () => {};
  [dynamic] = function own() {};
}
class Quoted { "constructor"() {} }
class Escaped { \u0063onstructor() {} }
function outer() { function inner() { return 1; } return 2; }
