type AnyFunction = (...args: any[]) => any;
enum E { A, B }
enum S { X = "x" }
const enum C { P, Q }
declare function k<T>(o: T): keyof T;
declare function ia<T>(o: T): T[keyof T];
declare function ex<T>(o: T): Exclude<T[keyof T], AnyFunction>;
declare const typeofE: typeof E;
declare const typeofS: typeof S;
declare const typeofC: typeof C;
E;
k(E);
ia(E);
ex(E);
typeofE;
typeofE[0];
E.A;
E[0];
S;
k(S);
ia(S);
ex(S);
typeofS;
typeofS[0];
S.X;
C;
k(C);
ia(C);
ex(C);
typeofC;
typeofC[0];
C.P;
