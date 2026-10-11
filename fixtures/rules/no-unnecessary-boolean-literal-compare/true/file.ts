declare const x: boolean | undefined;
if (x === true) {}
x === true;
x !== true;
!(x !== true);
declare const other: boolean;
if (other && x === true) {}
const value = other && x === true;
x === false;
if ((x === true) ?? true) {}
if (null ?? (x === true)) {}
