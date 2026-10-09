
        namespace custom {
            export interface Promise<T> { then(callback: (value: T) => unknown): unknown; finally(callback: () => void): void }
            declare const promise: Promise<number>;
            promise.finally(async () => {});
        }
    