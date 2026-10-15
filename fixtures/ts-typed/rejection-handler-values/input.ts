interface Like {
  then(ok: (value: unknown) => void, bad: (error: any) => void): Like;
  catch(bad: (error: any) => void): Like;
}
declare const promise: Like;
declare const flag: boolean;
type UnsafeRest = any[];
type SafeRest = unknown[];
promise.catch(error => {});
promise.catch((error: unknown) => {});
promise.catch((...errors: UnsafeRest) => {});
promise.catch((...errors: SafeRest) => {});
promise.catch((null, error => {}));
promise.catch(flag && ((error: any) => {}));
declare const ordinary: { catch(handler: (error: any) => void): void };
ordinary.catch((error: any) => {});
declare const existing: (error: any) => void;
promise.catch(existing);
promise.then((value: any) => {}, (error: unknown) => {});
promise.catch(...[(error: any) => {}]);
let method = 'catch';
promise[method]((error: any) => {});
let changedMethod = 'catch';
changedMethod = 'finally';
promise[changedMethod]((error: any) => {});
promise.catch((error: string | ((number | unknown) & { b: () => void })) => {});
promise.catch((...errors: [unknown[], string | ((number | unknown) & { b: () => void }), string]) => {});
