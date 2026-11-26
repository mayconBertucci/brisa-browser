import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { auditCenter } from "./verify-usability.mjs";

const sessionCookie = "brisa_fixture_session=active";
const providerCookie = "brisa_fixture_provider=active";
const requests = [];
const executable = resolve(process.argv[2] ?? "target/release/brisa.exe");
const historyScreenshot = process.argv[3];
const auditDirectory = process.argv[4] && resolve(process.argv[4]);
const temporaryRoot = resolve(tmpdir());
const workspace = await mkdtemp(join(temporaryRoot, "brisa-privacy-"));
const profile = join(workspace, "profile");
const rules = join(workspace, "fixture.txt");
const server = createServer(handle);
await writeFile(rules, "/tracker.js|$script\n/ad-popup|$popup\n");

server.listen(0, "127.0.0.1");
await once(server, "listening");
const testSite = "http://127.0.0.1:" + server.address().port;
const providerSite = "http://localhost:" + server.address().port;
if (auditDirectory) {
  await mkdir(auditDirectory, { recursive: true });
  await mkdir(profile, { recursive: true });
  await writeFile(join(profile, "brisa.json"), JSON.stringify({
    bookmarks: [{ title: "Referência de auditoria", url: testSite + "/session" }],
  }));
}

function handle(request, response) {
  const url = new URL(request.url, testSite);
  const cookies = request.headers.cookie?.split("; ") ?? [];
  const session = cookies.includes(sessionCookie);
  const provider = cookies.includes(providerCookie);
  requests.push({ path: url.pathname, session, provider });
  response.setHeader("Cache-Control", "no-store");
  const setCookie = (cookie, seconds) => {
    response.setHeader("Set-Cookie", cookie + "; Path=/; Max-Age=" + seconds + "; SameSite=Lax; HttpOnly");
  };
  const html = (title, body = "") => {
    response.setHeader("Content-Type", "text/html; charset=utf-8");
    response.end("<!doctype html><meta charset=utf-8><title>" + title + "</title>" + body);
  };
  const script = (body) => {
    response.setHeader("Content-Type", "text/javascript");
    response.end(body);
  };

  switch (url.pathname) {
    case "/sign-in":
      setCookie(sessionCookie, 600);
      html("Prova de privacidade", `
        <h1>Prova de privacidade do Brisa</h1>
        <button id="same-origin">Outra aba</button>
        <button id="direct">Login em outro site</button>
        <button id="blank">Login iniciado em branco</button>
        <button id="ad">Anúncio bloqueado</button>
        <script src="${providerSite}/functional.js"></script>
        <script src="${providerSite}/tracker.js"></script>
        <script>
          const provider = ${JSON.stringify(providerSite)};
          let loginTab;
          window.receipts = [];
          document.querySelector("#same-origin").onclick = () => window.open("/session?tab=second");
          document.querySelector("#direct").onclick = () => {
            loginTab = window.open(provider + "/authorize?flow=direct");
          };
          document.querySelector("#blank").onclick = () => {
            loginTab = window.open("about:blank");
            if (loginTab) loginTab.location.href = provider + "/authorize?flow=blank";
          };
          document.querySelector("#ad").onclick = () => window.open(provider + "/ad-popup");
          addEventListener("message", (event) => {
            if (event.origin === provider && event.source === loginTab) {
              window.receipts.push(event.data);
            }
          });
          addEventListener("load", () => {
            window.open(provider + "/automatic-popup");
            window.fixtureReady = true;
          });
        </script>
      `);
      break;
    case "/session":
      response.statusCode = session ? 200 : 401;
      html(session ? "Sessão ativa" : "Sem sessão");
      break;
    case "/logout":
      setCookie(sessionCookie, 0);
      html("Sessão encerrada");
      break;
    case "/authorize":
      setCookie(providerCookie, 600);
      html("Provedor de teste", `
        <script src="/tracker.js"></script>
        <script>
          window.opener?.postMessage(
            { flow: ${JSON.stringify(url.searchParams.get("flow"))}, tracker: Boolean(window.trackerRan) },
            ${JSON.stringify(testSite)}
          );
          setTimeout(() => window.close(), 250);
        </script>
      `);
      break;
    case "/functional.js":
      script("window.functionalLoaded = true;");
      break;
    case "/tracker.js":
      setCookie("brisa_fixture_tracker=active", 600);
      script("window.trackerRan = true;");
      break;
    default:
      response.statusCode = 404;
      response.end();
  }
}

async function until(action, description) {
  const deadline = Date.now() + 15000;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const value = await action();
      if (value) return value;
    } catch (error) {
      lastError = error;
    }
    await delay(80);
  }
  throw new Error("Tempo esgotado: " + description, { cause: lastError });
}

async function protocol(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let sequence = 0;
  socket.addEventListener("message", ({ data }) => {
    const response = JSON.parse(data);
    pending.get(response.id)?.(response);
  });
  await Promise.race([
    new Promise((opened, failed) => {
      socket.addEventListener("open", opened, { once: true });
      socket.addEventListener("error", failed, { once: true });
    }),
    delay(4000, undefined, { ref: false }).then(() => { throw new Error("Conexão de teste indisponível."); }),
  ]);
  return {
    close: () => socket.close(),
    async command(method, params) {
      const id = ++sequence;
      const reply = new Promise((done) => pending.set(id, done));
      try {
        socket.send(JSON.stringify({ id, method, params }));
        const { error, result } = await Promise.race([
          reply,
          delay(4000, undefined, { ref: false }).then(() => { throw new Error(method + " sem resposta."); }),
        ]);
        if (error) throw new Error(method + ": " + error.message);
        return result;
      } finally {
        pending.delete(id);
      }
    },
  };
}

async function inspect(client, expression) {
  const { result, exceptionDetails } = await client.command("Runtime.evaluate", {
    expression, returnByValue: true, awaitPromise: true,
  });
  assert.equal(exceptionDetails, undefined);
  return result.value;
}

async function click(client, selector) {
  const { x, y } = await inspect(client, `
    (() => {
      const {x, y, width, height} = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();
      return {x: x + width / 2, y: y + height / 2};
    })()
  `);
  for (const type of ["mousePressed", "mouseReleased"]) {
    await client.command("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
}

async function inBrowser(path, run) {
  const startupUrl = path.startsWith("/") ? testSite + path : path;
  const documentUrl = path.startsWith("brisa://") ? "https://brisa.invalid/center" : startupUrl;
  const portFile = join(profile, "EBWebView", "DevToolsActivePort");
  await rm(portFile, { force: true });
  const child = spawn(executable, ["--profile", profile, "--url", startupUrl, "--rules", rules], {
    env: {
      ...process.env,
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=0 --remote-debugging-address=127.0.0.1",
    },
    stdio: "ignore",
  });
  const exited = once(child, "exit");
  exited.catch(() => {});
  let browser;
  let page;
  try {
    const port = await until(async () => {
      const contents = await readFile(portFile, "utf8");
      return Number(contents.split("\n")[0]);
    }, "porta de teste do WebView2");
    const endpoint = "http://127.0.0.1:" + port;
    const getJson = async (suffix) => (await fetch(endpoint + suffix, {
      signal: AbortSignal.timeout(2500),
    })).json();
    const targets = () => getJson("/json/list");
    const initial = await until(async () => (await targets()).find(
      ({ url }) => url === documentUrl,
    ), "página " + path);
    const { webSocketDebuggerUrl } = await getJson("/json/version");
    browser = await protocol(webSocketDebuggerUrl);
    page = await protocol(initial.webSocketDebuggerUrl);
    return await run(page, targets);
  } finally {
    page?.close();
    if (browser) {
      await browser.command("Browser.close").catch(() => {});
      browser.close();
      await delay(850);
    }
    if (child.exitCode === null && child.signalCode === null) child.kill();
    await exited;
  }
}

try {
  const result = await inBrowser("/sign-in", async (page, targets) => {
    await until(() => inspect(page, "window.fixtureReady"), "página e recursos do site");
    assert.equal(await inspect(page, "window.functionalLoaded"), true);
    assert.equal(await inspect(page, "Boolean(window.trackerRan)"), false);
    assert.equal(await inspect(page, "fetch('/session').then(r => r.status)"), 200);

    await click(page, "#same-origin");
    await until(async () => (await targets()).some(({ url, title }) => (
      url === testSite + "/session?tab=second" && title === "Sessão ativa"
    )), "cookie de sessão compartilhado na segunda aba");

    for (const [index, flow] of ["direct", "blank"].entries()) {
      await click(page, "#" + flow);
      const receipt = await until(() => inspect(page, "window.receipts[" + index + "]"), "resposta do login " + flow);
      assert.deepEqual(receipt, { flow, tracker: false });
      await until(async () => !(await targets()).some(({ url }) => url.startsWith(providerSite + "/authorize")),
        "fechamento da aba pelo provedor");
    }
    const authorization = requests.filter(({ path }) => path === "/authorize");
    assert.deepEqual(authorization.map(({ provider }) => provider), [false, true]);
    await click(page, "#ad");
    await delay(500);
    const blocked = new Set(["/tracker.js", "/automatic-popup", "/ad-popup"]);
    assert.deepEqual(requests.filter(({ path }) => blocked.has(path)), []);
    return {
      session_shared_across_tabs: true,
      functional_cross_site_script_loaded: true,
      direct_and_blank_popups_returned_to_opener: true,
      provider_cookie_survived_between_popups: true,
      server_requests_for_tracker_or_blocked_popups: 0,
    };
  });
  await inBrowser("/session", async (page) => {
    const title = await until(() => inspect(page, "document.title"), "sessão após reiniciar");
    assert.equal(title, "Sessão ativa");
  });
  const history = await inBrowser("brisa://historico", async (page) => {
    const visits = "Array.from(document.querySelectorAll('.history-visit .name small'), element => element.textContent)";
    const before = await until(async () => {
      const entries = await inspect(page, visits);
      return entries.length > 2 && entries;
    }, "visitas no histórico");
    for (const index of [0, 1]) {
      await click(page, ".history-visit:nth-child(" + (index + 1) + ") input");
    }
    assert.equal(await inspect(page, "document.querySelector('.selection-count').textContent"), "2 selecionadas");
    if (historyScreenshot) {
      const { data } = await page.command("Page.captureScreenshot", { format: "png" });
      await writeFile(resolve(historyScreenshot), Buffer.from(data, "base64"));
    }
    await click(page, "#actions button.danger:not([hidden])");
    await click(page, "#confirm");
    await until(async () => (await inspect(page, visits)).length === before.length - 2, "exclusão de duas visitas");
    const after = await inspect(page, visits);
    assert.deepEqual(after, before.slice(2));
    return { visits_before: before.length, visits_removed: 2, visits_remaining: after.length };
  });
  await inBrowser("brisa://configuracoes", async (page) => {
    await until(() => inspect(page, "Boolean(document.querySelector('input[type=search]'))"), "busca nas configurações");
    assert.equal(await inspect(page, "document.querySelector('[name=search_engine]').value"), "google");
    await click(page, "input[type=search]");
    await page.command("Input.insertText", { text: "anuncios" });
    assert.deepEqual(await inspect(page,
      "Array.from(document.querySelectorAll('.setting:not([hidden]) strong'), el => el.textContent)"),
    ["Bloquear anúncios e rastreadores"]);
    await page.command("Input.insertText", { text: "xyz" });
    assert.equal(await inspect(page, "document.querySelector('#views [role=status]').hidden"), false);
    await inspect(page, `(() => {
      const search = document.querySelector("input[type=search]");
      search.value = "";
      search.dispatchEvent(new Event("input"));
      document.querySelector("[name=theme]").value = "dark";
      document.querySelector("[name=accent]").value = "violet";
      document.querySelector("[name=accent]").dispatchEvent(new Event("change"));
    })()`);
    await until(() => inspect(page, "document.documentElement.dataset.theme === 'dark' && document.documentElement.dataset.accent === 'violet'"), "tema e cor aplicados");
    await inspect(page, "Array.from(document.querySelectorAll('button')).find(button => button.textContent === 'Esvaziar cache').click()");
    await until(() => inspect(page, "document.querySelector('#feedback').textContent === 'Cache esvaziado.'"), "limpeza exclusiva de cache");
  });
  await inBrowser("brisa://configuracoes", async (page) => {
    await until(() => inspect(page, "document.querySelector('[name=theme]')?.value === 'dark' && document.querySelector('[name=accent]')?.value === 'violet'"), "tema persistente");
    const started = await inspect(page, "performance.timeOrigin");
    for (const [section, heading] of [
      ["historico", "Histórico"], ["marcadores", "Marcadores"],
      ["protecao", "Bloqueios"], ["configuracoes", "Configurações"],
    ]) {
      await inspect(page, `document.querySelector('[data-page="${section}"]').click()`);
      await until(async () =>
        await inspect(page, "document.querySelector('#page-title').textContent") === heading,
        "seção interna " + heading,
      );
      assert.equal(await inspect(page, "performance.timeOrigin"), started);
      assert.equal(await inspect(page, "location.href"), "https://brisa.invalid/center");
    }
  });
  await inBrowser("/session", async (page) => {
    const title = await until(() => inspect(page, "document.title"), "sessão após esvaziar cache");
    assert.equal(title, "Sessão ativa");
    await inspect(page, "fetch('/logout').then(r => r.status)");
    assert.equal(await inspect(page, "fetch('/session').then(r => r.status)"), 401);
  });
  const usability = auditDirectory ? await inBrowser("brisa://marcadores", (page, targets) =>
    auditCenter({ page, targets, inspect, until, auditDirectory, testSite, profile })) : undefined;
    console.log(JSON.stringify({
    privacy: { ...result, session_survived_restart: true, session_survived_cache_clear: true, logout_expired_session: true },
    history,
    settings_search: true,
    default_search_is_google: true,
    shared_internal_document: true,
    appearance_persisted: true,
    usability,
  }, null, 2));
} finally {
  server.close();
  server.closeAllConnections();
  assert.equal(dirname(resolve(workspace)), temporaryRoot);
  assert.ok(basename(workspace).startsWith("brisa-privacy-"));
  await rm(workspace, { recursive: true, force: true, maxRetries: 2, retryDelay: 200 });
}
