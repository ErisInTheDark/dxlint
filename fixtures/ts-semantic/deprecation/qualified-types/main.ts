namespace Api {
  /** @deprecated */
  export type Old = string;
  export type Fresh = string;
  export namespace Nested {
    /** @deprecated */
    export interface Old {}
    export interface Fresh {}
  }
}
type Direct = Api.Old;
type Nested = Api.Nested.Old;
type Fresh = Api.Fresh | Api.Nested.Fresh;
type Unknown = Missing.Old;
