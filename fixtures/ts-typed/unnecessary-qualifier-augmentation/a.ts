import * as Foo from './foo';
declare module './foo' {
  const x: Foo.T = 3;
}
