declare const optional: { hello?: string };
optional as { hello: string };
declare const nested: { inner: { name?: string } };
nested as { inner: { name: string } };
declare const required: { hello: string };
required as { hello?: string };
declare const opaque: unknown;
opaque as unknown;
