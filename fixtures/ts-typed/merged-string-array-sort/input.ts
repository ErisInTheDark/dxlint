function named(items: Array<string>) { items.sort(); }
function constrained<T extends Array<string>>(items: T) { items.sort(); }
function mixed(items: Array<string | number>) { items.sort(); }
