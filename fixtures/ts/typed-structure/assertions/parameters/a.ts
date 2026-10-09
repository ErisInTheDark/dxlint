function widening<T extends string | number, V extends T>(x: T, y: V) { y as T; x as T | boolean; }
