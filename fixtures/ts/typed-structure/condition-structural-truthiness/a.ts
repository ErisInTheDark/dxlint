declare const empty: {};
if (empty) {}
interface Shape { item: number }
declare const shape: Shape;
if (shape) {}
type Brand = { brand: true };
declare const numberBrand: number & Brand;
if (numberBrand) {}
declare const stringBrand: string & Brand;
if (stringBrand) {}
declare const zero: 0 & Brand;
if (zero) {}
declare const blank: "" & Brand;
if (blank) {}
declare const nonzero: 1 & Brand;
if (nonzero) {}
function naked<T>(value: T) {
  if (value) {}
}
function constrained<T extends {}>(value: T) {
  if (value) {}
}
