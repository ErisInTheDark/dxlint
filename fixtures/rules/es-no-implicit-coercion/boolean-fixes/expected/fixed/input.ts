declare const foo: number, bar: number;
Boolean(foo);
Boolean((foo, bar));
Boolean(foo + bar);
function shadow(Boolean: (value: number) => boolean) { return !!foo; }
!!/* keep */foo;
typeof Boolean(foo);
