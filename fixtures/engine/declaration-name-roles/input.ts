import { Foo, Bar } from 'foo';
function baz<Foo>(): Foo {}
baz<Bar>();

const Default = 1, NS = 2, Local = 3, Eq = 4, AliasName = 5, Face = 6;
import type Default from 'types';
import type * as NS from 'types';
import type { Type as Local } from 'types';
import type Eq = require('types');
type AliasName = string;
interface Face {}

const command = (): ParameterDecorator => { return () => {}; };
export class Decorated {
  method(@command() command: string) { console.log(command); }
  nested(@(() => { const command = 1; return command; })() command: string) { console.log(command); }
}
