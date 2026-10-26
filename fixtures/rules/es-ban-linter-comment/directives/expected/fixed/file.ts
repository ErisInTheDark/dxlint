/* dxlint-ignore-start es-no-console */
/* dxlint-ignore-end es-no-console */
// dxlint-ignore es-no-console
// eslint is great
// global configuration lives elsewhere
// dxlint-ignore es-no-console
const value = 1;
// dxlint-ignore es-no-explicit-any es-import-no-deprecated
let loose: any = value; // dxlint-ignore es-semi
/* dxlint-ignore-start shared-eol-last es-no-sync */
loose = 2;
/* dxlint-ignore-end shared-eol-last es-no-sync */
// dxlint-ignore es-header
export { loose };
loose = 3; // eslint-disable-next-line no-console
