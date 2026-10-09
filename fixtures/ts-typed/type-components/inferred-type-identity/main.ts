namespace Case294 {

type IsArray<T> = T extends any[] ? true : false;
type ActuallyDuplicated = IsArray<number> | IsArray<string>;
      
}
namespace Case317 {
type T = string[] | string[];
}
namespace Case334 {
type T = string[][] | string[][];
}
namespace Case351 {
type T = [1, 2, 3] | [1, 2, 3];
}
namespace Case368 {
type T = () => string | string;
}
namespace Case385 {
type T = () => null | null;
}
namespace Case402 {
type T = (arg: string | string) => void;
}
namespace Case419 {
type T = 'A' | 'A';
}
namespace Case436 {

type A = 'A';
type T = A | A;
      
}
namespace Case459 {

type A = 'A';
const a: A | A = 'A';
      
}
namespace Case107 {
type T = readonly string[] | string[];
}
namespace Case149 {
type T = Class<string> | Class<string>;
}
