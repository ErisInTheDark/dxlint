type Α = Banned; type B = NS.Banned;
type C = Box<Banned>;
class D implements Banned {}
interface E extends Banned {}
class F implements Box<Banned> {}
type G = Banned<any>;
const H = 1 as Banned;
