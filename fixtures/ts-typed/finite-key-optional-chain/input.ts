type Foo = { [key: string]: string; foo: "foo"; bar: "bar" } | null;
declare const foo: Foo;
declare const key: "foo" | "bar";
foo?.[key]?.trim();
declare const wide: string;
foo?.[wide]?.trim();
declare const mixed: "foo" | "other";
foo?.[mixed]?.trim();
