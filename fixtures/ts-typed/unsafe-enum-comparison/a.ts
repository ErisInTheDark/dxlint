enum Fruit { Apple, Banana }
enum Other { Apple, Banana }
enum Text { Apple = "apple", Banana = "banana" }
Fruit.Apple === 0;
Fruit.Apple < Other.Banana;
Text.Apple === "apple";
Fruit.Apple === Fruit.Banana;
Fruit.Apple === null;
Fruit.Apple === ({} as any);
Fruit.Apple === "apple";
function caseCheck(value: Fruit) {
  switch (value) { case 0: break; case Fruit.Banana: break; default: break; }
}
declare const literalOverlap: Fruit | 0;
literalOverlap === 0;
declare const numberOverlap: Fruit | number;
numberOverlap === 1;
declare const stringOverlap: Text | string;
stringOverlap === "apple";
declare const sharedEnum: Fruit | Other;
sharedEnum === Fruit.Banana;
declare const mixedPrimitive: number | string;
Fruit.Apple === mixedPrimitive;
Text.Apple === mixedPrimitive;
declare const noOverlap: Fruit | -1;
noOverlap === 1;
const alias = Text.Apple;
"banana" !== alias;
namespace First { export enum Same { A } }
namespace Second { export enum Same { A } }
First.Same.A === Second.Same.A;
First.Same.A === First.Same.A;
declare const neverValue: never;
Fruit.Apple === neverValue;
Fruit.Apple === undefined;
function shadowed() {
  enum Fruit { Apple, Banana }
  Fruit.Apple === Fruit.Banana;
  Fruit.Apple === 0;
}
