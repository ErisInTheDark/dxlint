interface Α</* keep generic */ T> { value: /* keep body */ T; }
interface B /* do not drop */ { value: string; }
interface C extends Base</* keep heritage */ T> { value: string; }
