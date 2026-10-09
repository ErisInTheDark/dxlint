type Result<T> = T extends string ? number : boolean; declare const mixed: Result<string | number>; mixed as boolean; declare const uncertain: Result<any>; uncertain as number;
