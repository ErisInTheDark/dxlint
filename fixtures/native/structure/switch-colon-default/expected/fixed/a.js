switch (value) {
  case 0: break;
  case (1): work(); break;
  case 2 /* before */ :/* after */work(); break;
  case 3/* before */: /* after */work(); break;
  case 4
  :
    work(); break;
  default:
}
({ field :1 });
label : work();
