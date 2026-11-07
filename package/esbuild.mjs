// An esbuild plugin mapping files through a project's `.dx` transforms, the
// same way `tsc` sees them: one long-lived `dxlint map` for the whole build.
import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const launcher = join(dirname(fileURLToPath(import.meta.url)), "bin", "dxlint");

function server() {
	const child = spawn(process.execPath, [launcher, "map"], { stdio: ["pipe", "pipe", "inherit"] });
	const waiting = new Map();
	let next = 1;
	let buffer = Buffer.alloc(0);
	child.stdout.on("data", chunk => {
		buffer = Buffer.concat([buffer, chunk]);
		for (;;) {
			const header = buffer.indexOf("\r\n\r\n");
			if (header === -1) return;
			const length = Number(/content-length:\s*(\d+)/i.exec(buffer.toString("latin1", 0, header))[1]);
			if (buffer.length < header + 4 + length) return;
			const message = JSON.parse(buffer.toString("utf8", header + 4, header + 4 + length));
			buffer = buffer.subarray(header + 4 + length);
			const { resolve, reject } = waiting.get(message.id);
			waiting.delete(message.id);
			if (message.error) reject(new Error(message.error.message));
			else resolve(message.result);
		}
	});
	child.on("exit", () => {
		for (const { reject } of waiting.values()) reject(new Error("dxlint map exited"));
		waiting.clear();
	});
	return {
		request(method, params) {
			return new Promise((resolve, reject) => {
				const id = next++;
				waiting.set(id, { resolve, reject });
				const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
				child.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
				child.stdin.write(body);
			});
		},
		close() {
			child.stdin.end();
		},
	};
}

/** Byte offset of a problem as esbuild's line and column. */
function location(file, content, start, length) {
	const before = Buffer.from(content).subarray(0, start).toString();
	const line = before.split("\n").length;
	const column = before.length - (before.lastIndexOf("\n") + 1);
	return { file, line, column, length, lineText: content.split("\n")[line - 1] ?? "" };
}

/**
 * @param {{ filter?: RegExp, readFile?: (path: string) => string | Uint8Array | Promise<string | Uint8Array> }} [options]
 * `filter`: files to map, by default `.wts`. `readFile`: a file's content as
 * the build should see it (an editor buffer, another checkout's view); by
 * default the disk's.
 */
export function dxlintTransform({ filter = /\.wts$/, readFile: readContent = path => readFile(path, "utf8") } = {}) {
	return {
		name: "dxlint-transform",
		setup(build) {
			let mapper;
			build.onStart(() => { mapper ??= server(); });
			build.onDispose(() => { mapper?.close(); });
			build.onLoad({ filter }, async args => {
				const read = await readContent(args.path);
				const content = typeof read === "string" ? read : Buffer.from(read).toString("utf8");
				const result = await mapper.request("transform", { fileName: args.path, content, sourceMap: true });
				const errors = result.diagnostics.map(problem => ({ text: problem.messageText, location: location(args.path, content, problem.start, problem.length) }));
				if (errors.length > 0) return { errors };
				const map = Buffer.from(result.sourceMap).toString("base64");
				return { contents: `${result.text}\n//# sourceMappingURL=data:application/json;base64,${map}`, loader: "ts", resolveDir: dirname(args.path) };
			});
		},
	};
}
