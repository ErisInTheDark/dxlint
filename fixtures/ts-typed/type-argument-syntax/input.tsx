function Button<T = string>() { return <div />; }
const closed = <Button<string> />;
const paired = <Button<string>></Button>;
const changed = <Button<number> />;
declare function tag<T = number>(parts: readonly string[], value: T): void;
tag<number>`${1}`;
tag<string>`${"x"}`;
tag</* keep */ number>`${1}`;
