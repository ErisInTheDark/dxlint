import {} from 'foo' with { }
export {} from 'bar' with { }
import 'side' with { }
export * from 'star' with { }
import { a as b } from 'other' with {type: 'json', mode: 'strict'}
