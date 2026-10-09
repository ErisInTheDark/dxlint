
            declare const x: number | undefined;
            declare function f(x: number): boolean;
            if (x || 1) {}
            if (!(x || 1)) {}
            if (f(x || 1)) {}
            if (+(x || 1)) {}
        