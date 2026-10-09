namespace Case294 {

type IsArray<T> = T extends any[] ? true : false;
type ActuallyDuplicated = IsArray<number>  ;
      
}
namespace Case317 {
type T = string[]  ;
}
namespace Case334 {
type T = string[][]  ;
}
namespace Case351 {
type T = [1, 2, 3]  ;
}
namespace Case368 {
type T = () => string  ;
}
namespace Case385 {
type T = () => null  ;
}
namespace Case402 {
type T = (arg: string  ) => void;
}
namespace Case419 {
type T = 'A'  ;
}
namespace Case436 {

type A = 'A';
type T = A  ;
      
}
namespace Case459 {

type A = 'A';
const a: A   = 'A';
      
}
namespace Case107 {
type T = readonly string[] | string[];
}
namespace Case149 {
type T = Class<string> | Class<string>;
}
