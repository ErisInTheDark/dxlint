const Α = 'x' as const; const B = 1 as const;
const C = <const>'y';
const D = <const>4;
const E = ('z') as const;
const F = 'a' as /* keep */ const;
const G = < /* left */ const /* right */ > 'b';
class H { field = 'c' as const; }
const I = '\u0061' as const;
