const asynchronous = {
  async [Symbol.dispose]() {},
};
using indirect = asynchronous;
using direct = {
  async [Symbol.dispose]() {},
};
using synchronous = {
  [Symbol.dispose]() {},
};
const stored = {
  async [Symbol.dispose]() {},
};
await using awaited = asynchronous;
await using asynchronousProtocol = {
  async [Symbol.asyncDispose]() {},
};
using ordinaryName = {
  async dispose() {},
};
function shadowed() {
  const Symbol = { dispose: "dispose" };
  using differentKey = {
    async [Symbol.dispose]() {},
  };
}
declare const uncertain: any;
using unknownProtocol = uncertain;
