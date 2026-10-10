const a = x => (test ? yes : no);
const b = (x, y) => test ? yes : no;
const c = ({ x }) => test ? yes : no;
const d = (x = 1) => test ? yes : no;
const e = (...xs) => test ? yes : no;
