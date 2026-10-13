class Base {
  cb() {}
}
class Inferred extends Base {
  get cb() {
    return () => 1;
  }
}
class Annotated extends Base {
  get cb(): () => void {
    return () => 2;
  }
}
class Valid extends Base {
  get cb() {
    return () => {};
  }
}
class Scalar extends Base {
  get cb() {
    return 3;
  }
  set cb(value: number) {}
}
