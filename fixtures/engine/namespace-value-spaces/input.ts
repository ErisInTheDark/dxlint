namespace Empty {}
Empty;
namespace Types { interface X {} type T = 1; }
Types;
namespace Nested { namespace Inner { type T = 1; } }
Nested;
namespace Runtime { export const value = 1; }
Runtime;
namespace Semi { ; }
Semi;
namespace ConstEnum { export const enum E { X } }
ConstEnum;
namespace Alias { import Private = Runtime; }
Alias;
namespace PublicAlias { export import Public = Runtime; }
PublicAlias;
