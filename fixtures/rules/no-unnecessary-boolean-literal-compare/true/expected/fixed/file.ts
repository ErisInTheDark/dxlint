declare const x: boolean | undefined;
if (x) {}
x ?? false;
!x;
x ?? false;
declare const other: boolean;
if (other && x) {}
const value = other && (x ?? false);
x === false;
if ((x ?? false) ?? true) {}
if (null ?? (x)) {}
