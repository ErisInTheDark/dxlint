class WrittenSymbol {
  private [Symbol.iterator] = 1;
  change() { this[Symbol.iterator] = 2; }
}
class UntouchedSymbol {
  private readonly [Symbol.iterator] = 1;
}
class StringLookalike {
  private readonly "[Symbol.iterator]" = 1;
  changeSymbol() { this[Symbol.iterator] = 2; }
}
class WrittenString {
  private "[Symbol.iterator]" = 1;
  change() { this["[Symbol.iterator]"] = 2; }
}
class ConstructorSymbol {
  private readonly [Symbol.iterator]: number = 1;
  constructor() { this[Symbol.iterator] = 2; }
}
declare const distinctSymbol: unique symbol;
class ShadowedSymbol {
  private readonly [Symbol.iterator] = 1;
  change() { const Symbol = { iterator: distinctSymbol }; this[Symbol.iterator] = 2; }
}
