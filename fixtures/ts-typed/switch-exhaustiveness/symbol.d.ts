interface SymbolConstructor {
  (description?: string): symbol;
  for(key: string): symbol;
}
declare const Symbol: SymbolConstructor;
