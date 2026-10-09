type Direct = { [key: string]: Direct };
interface First { [key: string]: Second; }
interface Second { [key: string]: First; }
type Mapped = { [key in string]: Mapped };
