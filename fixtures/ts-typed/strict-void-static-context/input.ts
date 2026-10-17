const arrow: { cb: () => void } = class {
  static cb = () => ({});
};
const method: { cb: () => void } = class {
  static cb() { return 1; }
};
const computed: { cb: () => void } = class {
  static ["cb"] = () => 1;
};
const instance: { cb: () => void } = class {
  cb = () => 1;
};
const number: { cb: () => number } = class {
  static cb = () => 1;
};
const explicit: { cb: () => void } = class {
  static cb: () => number = () => 1;
};
const noContext = class {
  static cb = () => 1;
};
const empty: { cb: () => void } = class {
  static cb = () => {};
};
