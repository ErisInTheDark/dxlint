export {};
enum E { Zero, One, Text = 'a' }
type Whole = E;
type Member = E.Zero;
type Query = typeof E.One;
const value = E.Text;
void E;
