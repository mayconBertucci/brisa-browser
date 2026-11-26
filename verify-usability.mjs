import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const colors = (value) => {
  value = value.trim();
  if (/^#[\da-f]{3}$/i.test(value)) return [...value.slice(1)].map((channel) => parseInt(channel.repeat(2), 16));
  if (/^#[\da-f]{6}$/i.test(value)) return [1, 3, 5].map((offset) => parseInt(value.slice(offset, offset + 2), 16));
  const channels = value.match(/^rgba?\(([\d.]+), ([\d.]+), ([\d.]+)(?:, 1)?\)$/);
  assert.ok(channels, "Cor esperada em sRGB opaco: " + value);
  return channels.slice(1).map(Number);
};

function contrast(first, second) {
  const luminance = (color) => colors(color).map((value) => {
    const channel = value / 255;
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
  const levels = [luminance(first), luminance(second)].sort((a, b) => b - a);
  return (levels[0] + 0.05) / (levels[1] + 0.05);
}

export async function auditCenter({ page, targets, inspect, until, auditDirectory, testSite, profile }) {
  const evaluate = (body) => inspect(page, body);
  const check = (expression, name) => until(() => evaluate(expression), name);
  const saveScreenshot = async (name) => {
    const { data } = await page.command("Page.captureScreenshot", { format: "png" });
    await writeFile(join(auditDirectory, name + ".png"), Buffer.from(data, "base64"));
  };
  const button = (title, scope = "document") => evaluate(`
    (() => {
      const element = Array.from((${scope}).querySelectorAll("button"))
        .find(button => button.textContent === ${JSON.stringify(title)} && !button.closest("[hidden]"));
      if (!element) throw new Error("Botão da auditoria ausente");
      element.click();
      return true;
    })()
  `);
  const rowFor = (title) => `Array.from(document.querySelectorAll(".row")).find(
    row => row.querySelector(".name > button")?.textContent === ${JSON.stringify(title)}
  )`;
  const typeIn = (selector, value) => evaluate(`
    (() => {
      const field = document.querySelector(${JSON.stringify(selector)});
      field.value = ${JSON.stringify(value)};
      field.dispatchEvent(new Event("input", { bubbles: true }));
      field.dispatchEvent(new Event("change", { bubbles: true }));
    })()
  `);
  const selectSection = async (name, title) => {
    await evaluate(`document.querySelector('[data-page="${name}"]').click()`);
    await check(`document.querySelector("#page-title").textContent === ${JSON.stringify(title)}`, title);
  };
  const readPreferences = async () =>
    JSON.parse(await readFile(join(profile, "brisa.json"), "utf8")).preferences;
  const testKey = async (key, code, virtualKey) => {
    for (const type of ["keyDown", "keyUp"]) {
      await page.command("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: virtualKey });
    }
  };

  await check("document.querySelector('#actions button')?.textContent === 'Nova pasta'", "gerenciador de marcadores");
  await button("Nova pasta");
  assert.equal(await evaluate("document.activeElement.id"), "form-title");
  await testKey("Escape", "Escape", 27);
  await check("!document.querySelector('#editor').open", "fechar diálogo com Escape");
  await button("Nova pasta");
  await typeIn("#form-title", "Pasta de auditoria");
  await button("Criar pasta");
  await check(`Boolean(${rowFor("Pasta de auditoria")})`, "criação de pasta");
  await button("Renomear", rowFor("Pasta de auditoria"));
  await typeIn("#form-title", "Referências");
  await button("Salvar");
  await check(`Boolean(${rowFor("Referências")})`, "renomear pasta");
  await button("Editar ou mover", rowFor("Referência de auditoria"));
  await typeIn("#form-title", "Página de referência");
  await evaluate(`Array.from(document.querySelector("#form-folder").options)
    .find(option => option.textContent === "Referências").selected = true`);
  await button("Salvar");
  await until(async () => {
    const saved = JSON.parse(await readFile(join(profile, "brisa.json"), "utf8"));
    return saved.bookmarks[0].title === "Página de referência" && saved.bookmarks[0].folder !== 0;
  }, "edição e movimentação de marcador");
  await typeIn(".search", "referencia");
  const bookmarks_search_accepts_unaccented_query = await evaluate(`Boolean(${rowFor("Página de referência")})`);
  await typeIn(".search", "referência");
  assert.ok(await evaluate(`Boolean(${rowFor("Página de referência")})`));
  await saveScreenshot("marcadores-escuro");

  await selectSection("configuracoes", "Configurações");
  const themes = [];
  for (const theme of ["light", "dark"]) {
    for (const accent of ["jade", "blue", "violet"]) {
      await typeIn("[name=theme]", theme);
      await typeIn("[name=accent]", accent);
      await check(`document.documentElement.dataset.theme === "${theme}"
        && document.documentElement.dataset.accent === "${accent}"`, theme + " " + accent);
      const sample = await evaluate(`
        (() => {
          const root = getComputedStyle(document.documentElement);
          const style = getComputedStyle(document.querySelector(".search"));
          const property = (name) => root.getPropertyValue(name).trim();
          return {
            surface: property("--surface"), base: property("--base"), ink: property("--text"),
            muted: property("--muted"), accent: property("--mint"), onAccent: property("--on-accent"),
            field: style.backgroundColor, placeholder: getComputedStyle(document.querySelector(".search"), "::placeholder").color,
          };
        })()
      `);
      themes.push({
        theme, accent,
        primary_text: contrast(sample.ink, sample.base),
        secondary_text: contrast(sample.muted, sample.surface),
        accent_text: contrast(sample.accent, sample.base),
        button_text: contrast(sample.onAccent, sample.accent),
        search_placeholder: contrast(sample.placeholder, sample.field),
      });
      if (accent === "jade") await saveScreenshot("configuracoes-" + theme);
    }
  }
  const keyboard = [];
  await evaluate("document.querySelector('nav button').focus()");
  for (let step = 0; step < 8; step++) {
    await testKey("Tab", "Tab", 9);
    keyboard.push(await evaluate(`
      (() => {
        const active = document.activeElement;
        const style = getComputedStyle(active);
        return {
          label: active.getAttribute("aria-label") || active.labels?.[0]?.textContent.trim() || active.textContent.trim(),
          focusVisible: active.matches(":focus-visible"),
          outline: style.outlineStyle,
        };
      })()
    `));
  }
  for (const [name, value] of [["search_engine", "bing"], ["search_engine", "brave"], ["search_engine", "duck_duck_go"], ["search_engine", "google"]]) {
    await typeIn(`[name="${name}"]`, value);
    await until(async () => (await readPreferences())[name] === value, "salvar " + value);
  }
  for (const name of ["show_bookmarks", "remember_history"]) {
    for (const checked of [false, true]) {
      await evaluate(`
        (() => {
          const input = document.querySelector('[name="${name}"]');
          if (input.checked !== ${checked}) input.click();
        })()
      `);
      await until(async () => (await readPreferences())[name] === checked, "salvar " + name);
    }
  }
  const layout = [];
  for (const width of [480, 320]) {
    await page.command("Emulation.setDeviceMetricsOverride", { width, height: 780, deviceScaleFactor: 1, mobile: false });
    await check(`innerWidth === ${width}`, "largura " + width);
    for (const [name, title] of [["configuracoes", "Configurações"], ["marcadores", "Marcadores"], ["historico", "Histórico"], ["protecao", "Bloqueios"]]) {
      await selectSection(name, title);
      layout.push(await evaluate(`
        ({
          width: innerWidth,
          section: document.querySelector("#page-title").textContent,
          pageOverflow: document.documentElement.scrollWidth - innerWidth,
          navOverflow: document.querySelector("nav").scrollWidth - document.querySelector("nav").clientWidth,
        })
      `));
    }
    await saveScreenshot("pagina-estreita-" + width);
  }
  await page.command("Emulation.clearDeviceMetricsOverride");
  await selectSection("marcadores", "Marcadores");
  await typeIn(".search", "referência");
  await button("Página de referência");
  await until(async () => (await targets()).some(({ url }) => url === testSite + "/session"), "abrir marcador");
  await button("Excluir", rowFor("Página de referência"));
  const bookmark_delete_opens_confirmation = await evaluate("document.querySelector('#editor').open");
  if (bookmark_delete_opens_confirmation) {
    await evaluate("document.querySelector('#confirm').click()");
  }
  await check(`!Boolean(${rowFor("Página de referência")})`, "excluir marcador");
  await typeIn(".search", "");
  await button("Excluir", rowFor("Referências"));
  assert.equal(await evaluate("document.querySelector('#editor').open"), true);
  await button("Excluir pasta");
  await check(`!Boolean(${rowFor("Referências")})`, "excluir pasta");
  assert.match(await evaluate("document.querySelector('.empty').textContent"), /estrela/);
  return {
    bookmarks: {
      create_rename_move_open_delete: true,
      accepts_unaccented_query: bookmarks_search_accepts_unaccented_query,
      delete_opens_confirmation: bookmark_delete_opens_confirmation,
      cancel_dialog_with_escape: true,
    },
    preferences: { four_search_engines_saved: true, bookmark_bar_and_history_flags_saved: true },
    themes,
    keyboard,
    layout,
  };
}
