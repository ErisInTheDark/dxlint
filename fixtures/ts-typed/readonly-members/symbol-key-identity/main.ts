class WrittenSymbol {
  private [Symbol.iterator] = 1;
  change() { this[Symbol.iterator] = 2; }
}
class UntouchedSymbol {
  private [Symbol.iterator] = 1;
}
class StringLookalike {
  private "[Symbol.iterator]" = 1;
  changeSymbol() { this[Symbol.iterator] = 2; }
}
class WrittenString {
  private "[Symbol.iterator]" = 1;
  change() { this["[Symbol.iterator]"] = 2; }
}
class ConstructorSymbol {
  private [Symbol.iterator] = 1;
  constructor() { this[Symbol.iterator] = 2; }
}
declare const distinctSymbol: unique symbol;
class ShadowedSymbol {
  private [Symbol.iterator] = 1;
  change() { const Symbol = { iterator: distinctSymbol }; this[Symbol.iterator] = 2; }
}
