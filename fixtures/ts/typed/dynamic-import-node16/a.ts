import direct from "./deprecated.js";

async function inspect() {
  const commonjs = await import("./deprecated.js");
  commonjs.default;
  commonjs.default.default;
  direct;
  const named = await import("./named.js");
  named.named;
  named.default.named;
  const forcedCommonjs = await import("./forced.cjs");
  forcedCommonjs.default;
  forcedCommonjs.default.default;
  const forcedModule = await import("./forced.mjs");
  forcedModule.default;
  const modulePackage = await import("./esm/deprecated.js");
  modulePackage.default;
  const ambient = await import("ambient");
  ambient.default;
  ambient.named;
  const assigned = await import("./assigned.cjs");
  assigned.default;
  assigned.named;
  assigned.default.named;
  const callable = await import("./callable.cjs");
  callable.default;
  callable.default();
  (callable.default!)();
  const constructor = await import("./class.cjs");
  constructor.default;
  new constructor.default();
  new (constructor.default!)();
}
