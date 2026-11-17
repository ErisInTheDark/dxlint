// An esbuild plugin mapping files through a project's `.dx` transforms, the
// same way `tsc` sees them: one long-lived `dxlint map` shared by every build
// in the process, spoken to in its own protocol (`map { path, text, sourceMap }`).
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
	let failure;
	const fail = error => {
		failure ??= error;
		for (const { reject } of waiting.values()) reject(failure);
		waiting.clear();
	};
	child.on("error", error => fail(new Error(`dxlint map failed: ${error.message}`)));
	child.stdin.on("error", error => fail(new Error(`dxlint map stopped reading: ${error.message}`)));
	child.on("exit", code => fail(new Error(`dxlint map exited (${code})`)));
	child.stdout.on("data", chunk => {
		buffer = Buffer.concat([buffer, chunk]);
		for (;;) {
			const header = buffer.indexOf("\r\n\r\n");
			if (header === -1) return;
			const length = Number(/content-length:\s*(\d+)/i.exec(buffer.toString("latin1", 0, header))[1]);
			if (buffer.length < header + 4 + length) return;
			const message = JSON.parse(buffer.toString("utf8", header + 4, header + 4 + length));
			buffer = buffer.subarray(header + 4 + length);
			const pending = waiting.get(message.id);
			if (!pending) continue;
			waiting.delete(message.id);
			if (message.error) pending.reject(new Error(message.error.message));
			else pending.resolve(message.result);
		}
	});
	return {
		get alive() {
			return failure === undefined;
		},
		request(method, params) {
			return new Promise((resolve, reject) => {
				if (failure) return reject(failure);
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
function location(file, content, start, end) {
	const bytes = Buffer.from(content);
	const before = bytes.subarray(0, start).toString();
	const line = before.split("\n").length;
	const column = before.length - (before.lastIndexOf("\n") + 1);
	const length = bytes.subarray(start, end).toString().length;
	return { file, line, column, length, lineText: content.split("\n")[line - 1] ?? "" };
}

/** The process every mapper shares, and how many mappers are open. */
let shared;
let open = 0;

/** The shared process, started again if it died. */
function sharedServer() {
	if (!shared?.alive) shared = server();
	return shared;
}

/**
 * The project's `.dx` transforms as a function, for builds whose own `onLoad`
 * does more with a file. Every open mapper shares one long-lived `dxlint map`,
 * started on first use and ended when the last one closes; it maps each file
 * on its own and picks up `.dx` edits, so any number of builds may use it.
 * `transform` gives TypeScript `contents` with an inline source map, or esbuild
 * `errors` (and no `contents`) when the transform reports problems.
 * @returns {{ transform(fileName: string, content: string): Promise<{ contents?: string, errors: { text: string, location: object }[] }>, close(): void }}
 */
export function dxlintMapper() {
	let opened = false;
	return {
		async transform(fileName, content) {
			if (!opened) {
				opened = true;
				open++;
			}
			const result = await sharedServer().request("map", { path: fileName, text: content, sourceMap: true });
			const errors = result.problems.map(problem => ({ text: problem.message, location: location(fileName, content, problem.offset, problem.end_offset) }));
			if (errors.length > 0) return { errors };
			const map = Buffer.from(result.sourceMap).toString("base64");
			return { contents: `${result.text}\n//# sourceMappingURL=data:application/json;base64,${map}`, errors };
		},
		close() {
			if (!opened) return;
			opened = false;
			if (--open > 0) return;
			shared?.close();
			shared = undefined;
		},
	};
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
			const mapper = dxlintMapper();
			build.onDispose(() => mapper.close());
			build.onLoad({ filter }, async args => {
				const read = await readContent(args.path);
				const content = typeof read === "string" ? read : Buffer.from(read).toString("utf8");
				const { contents, errors } = await mapper.transform(args.path, content);
				return contents === undefined ? { errors } : { contents, loader: "ts", resolveDir: dirname(args.path) };
			});
		},
	};
}
