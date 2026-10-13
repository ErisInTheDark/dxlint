enum E {
  Zero,
  One,
  Ten = 0xa,
  Eleven,
  Negative = -1,
  Text = 'a',
  Computed = 1 + 2,
  Following,
  Known = 3,
  Alias = Known,
}
type Whole = E;
type Zero = E.Zero;
type One = E.One;
type Ten = E.Ten;
type Eleven = E.Eleven;
type Negative = E.Negative;
type Text = E.Text;
type Computed = E.Computed;
type Following = E.Following;
type Known = E.Known;
type Alias = E.Alias;
type Missing = E.Missing;
type Query = typeof E.One;
const zero = E.Zero;
const text = E.Text;
const computed = E.Computed;
void E;
