declare const foo: number, bar: number;
!!foo;
!!(foo, bar);
!!(foo + bar);
function shadow(Boolean: (value: number) => boolean) { return !!foo; }
!!/* keep */foo;
typeof!!foo;
