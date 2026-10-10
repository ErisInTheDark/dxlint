type Α</* keep generic */ T> = { value: /* keep body */ T; }
interface B /* do not drop */ { value: string; }
type C = { value: string; } & Base</* keep heritage */ T>
