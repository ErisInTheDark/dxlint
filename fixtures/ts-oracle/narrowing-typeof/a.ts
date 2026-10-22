declare const value: string | number;
value;
typeof value;
if (typeof value === "string") {
  value;
} else {
  value;
}
if (typeof value !== "string") {
  value;
}
declare const maybeUndefined: string | undefined;
if (typeof maybeUndefined === "undefined") {
  maybeUndefined;
} else {
  maybeUndefined;
}
declare const maybeNull: { a: number } | null;
if (typeof maybeNull === "object") {
  maybeNull;
} else {
  maybeNull;
}
declare const flag: boolean | string;
if (typeof flag === "boolean") {
  flag;
} else {
  flag;
}
function inspect(input: string | number | undefined) {
  if (typeof input === "object") {
    input;
  } else {
    input;
  }
  if (typeof input === "undefined") {
    input;
  }
  switch (typeof input) {
    case "string":
      input;
      break;
    default:
      input;
  }
}
typeof inspect;
