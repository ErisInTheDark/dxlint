declare const text: string;
declare const branded: string & { brand: true };
String(text);
String(branded);
function bounded<T extends string>(value: T) {
  String(value);
  value.toString();
}
function numeric<T extends number>(value: T) {
  Number(value);
  +value;
}
enum Label { First = "first", Second = "second" }
String(Label.First);
Label.First.toString();
enum Count { First = -1, Second = 2 }
Number(Count.First);
~~Count.First;
declare const integer: -2 | 3;
declare const fraction: -2 | 3.5;
~~integer;
~~fraction;
{
  const String = (value: string) => value;
  String(text);
}
String();
declare const mixed: string | number;
String(mixed);
