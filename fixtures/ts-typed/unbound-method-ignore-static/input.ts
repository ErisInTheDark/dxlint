class Methods {
  instance(): void {}
  static method(): void {}
  static field = function() {};
  static arrow = () => {};
}
const method = Methods.method;
const field = Methods.field;
const arrow = Methods.arrow;
const instance = new Methods().instance;
