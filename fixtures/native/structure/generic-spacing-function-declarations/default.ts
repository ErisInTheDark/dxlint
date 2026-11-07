function spaced <T>(value: T): T {
	return value;
}
function tight<T>(value: T): T {
	return value;
}
declare function declared <T>(value: T): T;
function overloaded <T>(value: T): T;
function overloaded <T>(value: T): T {
	return value;
}
export function exported <T>(value: T): T {
	return value;
}
const expression = function named <T>(value: T): T {
	return value;
};
const arrowed = () => function inner <T>(value: T): T {
	return value;
};
class Box <T> {
	value?: T;
}
export { spaced, tight, declared, overloaded, expression, arrowed, Box };
