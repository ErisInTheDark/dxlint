import { Box, make, overloaded, type Shape, type Alias } from "./base";
new Box<string>();
make<string>();
type S = Shape<string>;
type A = Alias<string>;
new Box<number>();
make<number>();
type Other = Shape<number>;
overloaded<string>("not the first overload");
