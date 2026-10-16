interface OpaqueCall { (): never; }
declare const opaque: OpaqueCall;
declare function plain(): never;
declare function generic<T>(): never;
opaque() as never;
plain() as never;
generic() as never;
