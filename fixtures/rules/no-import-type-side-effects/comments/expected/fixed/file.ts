import type /* header */ { /* name */ A, B /* alias */ as C } from 'a';
import type {
  // keep name
  D,
  /* before */ E,
} from 'b';
import type { F } from 'c' with { type: 'json' };
