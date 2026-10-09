
export class ClassWithDeprecatedConstructor {
    constructor();
    /** @deprecated */
    constructor(arg: string);
    constructor(arg?: string) {}
}
/** @deprecated */
export class DeprecatedClass {
    constructor();
    constructor(arg: string);
    constructor(arg?: string) {}
}
