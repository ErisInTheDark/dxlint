import { Box, make, overloaded, type Shape, type Alias } from "./base";
new Box();
make();
type S = Shape;
type A = Alias;
new Box<number>();
make<number>();
type Other = Shape<number>;
overloaded<string>("not the first overload");
