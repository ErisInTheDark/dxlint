type Freeze<T> = { +readonly [P in keyof T]: T[P] };
type Mutable<T> = { -readonly [P in keyof T]: T[P] };
type Optional<T> = { [P in keyof T]+?: T[P] };
type Complete<T> = { [P in keyof T]-?: T[P] };
function frozen(arg: Freeze<{ value: number }>) {}
function thawed(arg: Mutable<Freeze<{ value: number }>>) {}
function shallow(arg: Freeze<{ child: { value: number } }>) {}
function choice(arg: Freeze<{ value: number }> | { value: number }) {}
function optional(arg: Optional<Freeze<{ value: number }>>) {}
function complete(arg: Complete<Optional<Freeze<{ value: number }>>>) {}
