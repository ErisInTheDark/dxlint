const foo = Promise;
foo.reject();
new foo((resolve, reject) => reject(5));
new Promise((resolve, reject) => reject(5));
