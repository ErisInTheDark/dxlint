
        interface Awaitable<T> { then(callback: (value: T) => unknown): unknown }
        function f<T extends Awaitable<number>>(custom: Awaitable<number>, constrained: T) {
            if (custom) {}
            if (constrained) {}
        }
        function safe(ordinary: { then: string }, unknown: unknown, anything: any) {
            if (ordinary) {}
            if (unknown) {}
            if (anything) {}
        }
    