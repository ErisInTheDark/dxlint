switch (x) {
  case 1: break;
  default: break;
  case 1: break;
  case (1): break;
  case "1": break;
  case 0x1: break;
}
switch (x) {
  case obj.field: break;
  case obj /* gap */ . field: break;
  case fn(a + b): break;
  case fn(a+b): break;
  case fn((a + b)): break;
  case fn(ab): break;
  case "a b": break;
  case "ab": break;
  case 'a b': break;
}
switch (x) {
  case value: switch (y) {
    case value: break;
    case value: break;
  }
  case value: break;
}
switch (y) { case value: break; }
