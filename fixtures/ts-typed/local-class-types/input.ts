type Truth = false;
function inspect() {
  type Truth = true;
  class Local {
    readonly marker: true = true;
    static readonly flag: false = false;
    self(): Local { return this; }
    truth(value: Truth): Truth { return value; }
  }
  const local = new Local();
  if (local.self().marker) {}
  if (Local.flag) {}
  if (local.truth(true)) {}
}
function shadow() {
  class Local { readonly marker: false = false; }
  if (new Local().marker) {}
}
