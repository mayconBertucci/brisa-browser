import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInThisContext } from "node:vm";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import { parentPort, workerData } from "node:worker_threads";

const location = new URL(workerData);
globalThis.require = createRequire(location);
globalThis.__dirname = dirname(fileURLToPath(location));
globalThis.self = globalThis;
globalThis.location = location;
globalThis.addEventListener = (type, callback) => parentPort.on(type, data => callback({ data }));
globalThis.postMessage = message => parentPort.postMessage(message);
globalThis.importScripts = name => {
  const script = new URL(name, location);
  runInThisContext(readFileSync(script, "utf8"), { filename: fileURLToPath(script) });
};
globalThis.fetch = async url => new Response(readFileSync(url), {
  headers: { "Content-Type": "application/wasm" },
});
await import(location.href);
