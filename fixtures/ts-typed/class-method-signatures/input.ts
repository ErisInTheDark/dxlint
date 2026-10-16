class Service {
pick(value: string): string;
pick(value: number): number;
pick(value: string | number): string | number { return value; }
plain(value: number): number { return value; }
check(): void {
this.pick(1);
this.plain(1);
}
}
declare const service: Service;
service.pick("");
service.plain(1);
export {};
