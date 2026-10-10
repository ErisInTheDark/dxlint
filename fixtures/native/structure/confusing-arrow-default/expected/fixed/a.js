const a = x => (test ? /* keep */ yes : no);
const b = x => (test ? yes : no);
const nested = x => (test ? y => (other ? one : two) : three);
