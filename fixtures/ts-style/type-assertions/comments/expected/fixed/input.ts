const α = <Foo /* inside type */>bar(/* inside value */);
const b = <Foo> /* between */ bar;
const c = < /* before type */ Foo>bar;
const d = <Foo /* repeated */>bar(/* repeated */);
const e = bar(/* value */) as Foo</* preserved */ T>;
