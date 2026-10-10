type Α /* keep before equals */ = { value: /* keep body */ string };
type B = /* do not drop */ { value: string };
type C = ({ value: string } /* do not drop */);
type D</* keep generic */ T> = { value: T };
type E /* repeated */ = { value: string /* repeated */ };
