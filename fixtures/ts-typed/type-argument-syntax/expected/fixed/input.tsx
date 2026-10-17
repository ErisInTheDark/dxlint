function Button<T = string>() { return <div />; }
const closed = <Button />;
const paired = <Button></Button>;
const changed = <Button<number> />;
declare function tag<T = number>(parts: readonly string[], value: T): void;
tag`${1}`;
tag<string>`${"x"}`;
tag</* keep */ number>`${1}`;
