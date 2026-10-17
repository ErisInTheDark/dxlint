interface OpaqueCall { (): never; }
declare const opaque: OpaqueCall;
declare function plain(): never;
declare function generic<T>(): never;
opaque();
plain();
generic() as never;
