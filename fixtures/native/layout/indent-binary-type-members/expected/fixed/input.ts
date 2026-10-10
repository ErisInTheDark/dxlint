type Foo =
  | A | C
  | B;
type Pair =
  & A
  & (B
    | A
    | D);
type Args = Pick<Bar,
  Baz
  | Qux,
>;
