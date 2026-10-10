function named<T>() {}
class Box<T> {}
type Item<T> = T;
const called = fn<T>();
const built = new Box<T>();
const tagged = tag<T>`x`;
interface API { method<T>(): T; <T>(): T; }
type Factory = <T>() => T;
const anonymous = function <T>() {};
const arrow = <T>() => 1;
