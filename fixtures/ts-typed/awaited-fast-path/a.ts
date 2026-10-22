export {};

declare const pn: Promise<number>;
declare const ps: Promise<string>;
declare const nested: Promise<Promise<boolean>>;
declare const nestedUnion: Promise<Promise<string> | number>;
declare const both: Promise<number> | Promise<string>;
declare const mixed: Promise<number> | string;
declare const pl: PromiseLike<number>;
declare const custom: MyThenable<boolean>;
declare const inter: Promise<number> & { tag: string };
declare const pvoid: Promise<void>;
declare const pnull: Promise<null>;

async function probes() {
  const v1 = await pn;
  v1;

  const v2 = await ps;
  v2;

  const v3 = await nested;
  v3;

  const v4 = await nestedUnion;
  v4;

  const v5 = await both;
  v5;

  const v6 = await mixed;
  v6;

  const v7 = await pl;
  v7;

  const v8 = await custom;
  v8;

  const v9 = await inter;
  v9;

  const v10 = await pvoid;
  v10;

  const v11 = await pnull;
  v11;
}
