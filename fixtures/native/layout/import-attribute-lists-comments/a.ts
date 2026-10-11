import "a" with {/*head*/type:"json"/*tail*/};
export * from "b" with {/*head*/
type:"json", mode:"strict"/*tail*/};
import "c" with {/*empty*/};
