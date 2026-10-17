function booleans(other: boolean, inhabited: boolean) {
  if (other === inhabited) return;
  inhabited;
}
function numbers(other: number, inhabited: number) {
  if (other === inhabited) return;
  inhabited;
}
function literals(input: 1 | 2) {
  if (input === 1) {
    const keptThen: 1 = input;
  } else {
    const keptElse: 2 = input;
  }
}
function undefinedValue(input: number | undefined) {
  if (input === undefined) {
    const keptUndefined: undefined = input;
  } else {
    const keptDefined: number = input;
  }
}
function nullValue(input: number | null) {
  if (input === null) {
    const keptNull: null = input;
  } else {
    const keptNumber: number = input;
  }
}
export {};
