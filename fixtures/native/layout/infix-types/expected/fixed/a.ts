type A = string | number;
type B = Foo & Bar;
type C<T> = T extends U ? V : W;
type D<T = string> = T;
enum E{A = 1,B = 2}
class C{x = 1;}
function f(x = 1) {}
const {x = 1} = obj;
let a = 1; a += 2;
