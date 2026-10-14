throw 1;
throw new Error();
throw error;
throw getError();
throw holder.error;
throw holder[key];
throw tag``;
throw await pending;
function* generator() { throw yield error; }
throw (error = 1);
throw (error = other);
throw (error &&= 1);
throw (error &&= other);
throw (error ||= 1);
throw (error ??= 1);
throw (error += other);
throw (error && 1);
throw (1 && error);
throw (error || 1);
throw (1 ?? error);
throw (1 || 2);
throw (condition ? error : 1);
throw (condition ? 1 : error);
throw (condition ? 1 : 2);
throw (error, 1);
throw (1, error);
throw ((error));
throw undefined;
throw \u0075ndefined;
function shadow(undefined) { throw ((undefined)); }
Promise.reject(undefined);
Promise.reject(\u0075ndefined);
function rejectShadow(undefined) { Promise.reject((undefined)); }
Promise.reject(error && 1);
Promise.reject(1 && error);
throw holder?.error;
throw {};
throw (() => error);
