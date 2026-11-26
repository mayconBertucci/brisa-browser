import { BatchTranslator, TranslatorBacking } from "@browsermt/bergamot-translator/translator.js";
import { createHash } from "node:crypto";
import { mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { sep } from "node:path";
import { pathToFileURL } from "node:url";
import { Worker } from "node:worker_threads";

// Bergamot's published worker uses browser globals. Supply those globals
// in a Node worker without modifying the installed package.
globalThis.Worker = class {
  #worker;

  constructor(module) {
    this.#worker = new Worker(new URL("./translation-worker.mjs", import.meta.url), {
      workerData: module.href,
    });
  }

  addEventListener(name, callback) {
    this.#worker.on(name, data => callback({ data }));
  }

  postMessage(message) {
    this.#worker.postMessage(message);
  }

  terminate() {
    return this.#worker.terminate();
  }
};

const deadline = setTimeout(() => process.exit(1), 90_000);
const models = JSON.parse(await readFile(new URL("./translation-models.json", import.meta.url), "utf8"));
const directory = process.argv[2]
  ? pathToFileURL(process.argv[2] + sep)
  : new URL("./models/", import.meta.url);
const registry = "https://storage.googleapis.com/bergamot-models-sandbox/0.3.3/";

class LocalModels extends TranslatorBacking {
  async loadModelRegistery() {
    return Object.entries(models).map(([pair, files]) => ({ from: pair.slice(0, 2), to: pair.slice(2), files }));
  }

  async fetch(name, hash, { signal } = {}) {
    const entry = Object.entries(models).flatMap(([pair, files]) => Object.values(files).map(file => ({ pair, file })))
      .find(({ file }) => file.name === name && file.expectedSha256Hash === hash);
    if (!entry) throw new Error("Modelo ausente do catálogo do Brisa.");
    const { pair, file: model } = entry;
    const matches = bytes => bytes.byteLength === model.size && createHash("sha256").update(bytes).digest("hex") === hash;
    const cached = new URL(hash, directory);
    try {
      if ((await stat(cached)).size === model.size) {
        const bytes = await readFile(cached);
        if (matches(bytes)) return Uint8Array.from(bytes).buffer;
      }
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    const timeout = AbortSignal.timeout(48_000);
    const response = await fetch(new URL(pair + "/" + name, registry), {
      credentials: "omit", redirect: "error", signal: signal ? AbortSignal.any([timeout, signal]) : timeout,
    });
    if (!response.ok || !response.body) throw new Error("Não foi possível baixar o modelo.");
    const chunks = [];
    let size = 0;
    for await (const chunk of response.body) {
      size += chunk.byteLength;
      if (size > model.size) throw new Error("Modelo acima do tamanho esperado.");
      chunks.push(chunk);
    }
    const bytes = Buffer.concat(chunks, size);
    if (!matches(bytes)) throw new Error("O arquivo do modelo não passou na verificação.");
    await mkdir(directory, { recursive: true });
    const temp = new URL(hash + "." + process.pid + ".part", directory);
    await writeFile(temp, bytes);
    await rename(temp, cached);
    return Uint8Array.from(bytes).buffer;
  }
}

const onerror = error => {
  console.error(error?.data ?? error);
  process.exit(1);
};
const translator = new BatchTranslator({ workers: 1, onerror }, new LocalModels());
try {
  const chunks = [];
  let bytes = 0;
  for await (const chunk of process.stdin) {
    bytes += chunk.length;
    if (bytes > 160_000) throw new Error("Texto acima do limite da página.");
    chunks.push(chunk);
  }
  const { source, target, text } = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  const translated = await Promise.all(text.map(async (phrase) => {
    const prefix = phrase.match(/^\s*/u)[0];
    const suffix = phrase.match(/\s*$/u)[0];
    const result = await translator.translate({ from: source, to: target, text: phrase.trim() });
    return prefix + result.target.text + suffix;
  }));
  process.stdout.write(JSON.stringify(translated));
} finally {
  clearTimeout(deadline);
  translator.delete();
}
