const α = <Foo /* inside type */>bar(/* inside value */);
const b = <Foo> /* between */ bar;
const c = < /* before type */ Foo>bar;
const d = <Foo /* repeated */>bar(/* repeated */);
const e = <Foo</* preserved */ T>>bar(/* value */);
