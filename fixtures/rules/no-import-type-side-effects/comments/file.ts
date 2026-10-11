import /* header */ { type /* name */ A, type B /* alias */ as C } from 'a';
import {
  type // keep name
  D,
  /* before */ type E,
} from 'b';
import { type F } from 'c' with { type: 'json' };
