type NonNullable<T> = T & {}; type T = unknown; type Identity<T> = T; type Data = { a: string }; type Result = NonNullable<Identity<Data[keyof Data]>>;
