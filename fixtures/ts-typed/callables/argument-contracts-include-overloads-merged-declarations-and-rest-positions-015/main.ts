interface Take { (cb: () => void): void; (cb: () => Promise<void>): void } declare const take: Take; take(async () => {});
interface Elsewhere { (name: string, cb: () => void): void; (cb: () => Promise<void>): void } declare const elsewhere: Elsewhere; elsewhere('', async () => {});
