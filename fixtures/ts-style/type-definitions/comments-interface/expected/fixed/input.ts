interface Α /* keep before equals */ { value: /* keep body */ string }
type B = /* do not drop */ { value: string };
type C = ({ value: string } /* do not drop */);
interface D</* keep generic */ T> { value: T }
interface E /* repeated */ { value: string /* repeated */ }
