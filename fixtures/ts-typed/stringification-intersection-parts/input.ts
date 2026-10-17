class Left { left: string }
class Right { right: string }
declare const joined: Left & Right;
String(joined);
declare const array: Left[] & Right[];
array.toString();
array.join("");
declare const tuple: [Left, string] & [Left, Left];
String(tuple);
tuple.join("");
declare const custom: Left & { toString(): string };
String(custom);
declare const symbolCustom: Left & { [Symbol.toPrimitive](): string };
String(symbolCustom);
declare const primitive: string & {};
String(primitive);
interface EmptyLeft {}
interface EmptyRight {}
declare const empty: EmptyLeft & EmptyRight;
String(empty);
declare const arrayCustom: Left[] & { toString(): string };
String(arrayCustom);
