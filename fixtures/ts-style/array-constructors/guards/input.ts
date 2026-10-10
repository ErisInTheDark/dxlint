Array(1); new Array(9); Array?.(x);
Array<string>(); new Array<number>(1, 2);
foo.Array(); Array.foo();
function shadow(Array: any) { Array(); new Array(1, 2); }
