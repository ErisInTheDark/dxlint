class Foo { bazz() {} }
declare const foo: Foo;
foo[`ba${'zz'}`];
const fixed = 'zz';
foo[`ba${fixed}`];
let varying = 'zz';
varying = 'other';
foo[`ba${varying}`];
declare const unknown: string;
foo[`ba${unknown}`];
