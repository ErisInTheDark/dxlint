function declared(a: number) {
  return a;
}
var expression = function (a) {
  return a;
};
var arrow = () => 1;
class Sample {
  constructor() {}
  get value() {
    return 1;
  }
  static method(a) {
    return a;
  }
  field = (a) => a;
}
var object = {
  method(a) {
    return a;
  },
  field: (a) => a,
};
