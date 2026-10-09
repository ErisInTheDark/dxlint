
            declare function getArray<T extends number[]>(): T;
            delete getArray()[0];
            function remove<T extends number[]>(a: T) { delete a[0]; }
            function unknown<T>(a: T) { delete a[0]; }
        