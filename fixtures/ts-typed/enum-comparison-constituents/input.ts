enum Num { A = 1, B = 2 }
enum Str { A = 'a', AB = 'ab' }
declare const num: Num;
declare const str: Str;
str === 'a' + 'b';
1 + 1 === num;
declare const plainNumber: number & {};
plainNumber === Num.A;
declare const brandedNumber: number & { brand: void };
brandedNumber === Num.A;
declare const plainString: string & {};
plainString === Str.A;
declare const brandedString: string & { brand: void };
brandedString === Str.A;
type Weird = (string & { brand: void }) | (void & { brand: void }) | Str;
declare const weird: Weird;
weird === 'arbitrary';
declare const wide: Num | number;
wide === 1;
declare const same: Num | 1;
same === 1;
enum Auto { A, B }
Auto.A === 0;
declare const brandedUnion: Num | (number & { brand: void });
declare const plain: number;
brandedUnion === plain;
declare const unresolved: Missing;
unresolved === Num.A;
type Brand = number & { brand: void };
declare const enumOrBrand: Num | Brand;
declare const sameBrand: Brand;
enumOrBrand === sameBrand;
declare const otherBrand: 1 & { other: void };
enumOrBrand === otherBrand;
