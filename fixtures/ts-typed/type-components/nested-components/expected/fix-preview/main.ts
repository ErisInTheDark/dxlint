namespace Case667 {

type A = 'A';
type B = 'B';
type T = (A | B)  ;
      
}
namespace Case692 {

type A = 'A';
type T = A  ;
      
}
namespace Case715 {

type A = 'A';
type B = 'B';
type C = 'C';
type D = 'D';
type F = (A | B)   | ((C | D) & (A | B))  ;
      
}
namespace Case755 {

type A = 'A';
type B = 'B';
type C = (A | B)      ;
      
}
namespace Case802 {
type A = (number | string)    ;
}
namespace Case830 {
type A = (number | (string | null))  ;
}
namespace Case847 {
type A = (number & string)    ;
}
namespace Case875 {
type A = number & string & (  string);
}
namespace Case110 {

type A = 'A';
type B = 'B';
type C = 'C';
type D = 'D';
type T = (A | B) | (C | D);
      
}
namespace Case119 {

type A = 'A';
type B = 'B';
type T = (A | B) | (A & B);
      
}
