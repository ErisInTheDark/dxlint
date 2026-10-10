declare global {
  interface Array<T> { foo(value: T): T[]; }
  namespace Foo { interface Bar {} }
}
namespace Foo { interface Bar {} }
