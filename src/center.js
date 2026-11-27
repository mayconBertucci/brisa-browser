"use strict";

const bridge = window.chrome.webview;
const titles = { marcadores: "Marcadores", historico: "Histórico", configuracoes: "Configurações", protecao: "Bloqueios" };
const sectionButtons = document.querySelectorAll("nav button");
let section = null;
const views = document.querySelector("#views");
const actions = document.querySelector("#actions");
const feedback = document.querySelector("#feedback");
const editor = document.querySelector("#editor");
const form = document.querySelector("#editor-form");
const formTitle = document.querySelector("#form-title");
const formFolder = document.querySelector("#form-folder");
let submit = null;
let model = null;
let currentFolder = 0;
let query = "";

function send(action, values = {}) {
  bridge.postMessage({ action, section, ...values });
}

function create(tag, text = "", css = "") {
  const element = document.createElement(tag);
  element.textContent = text;
  if (css) element.className = css;
  return element;
}

function button(text, action, css = "") {
  const element = create("button", text, css);
  element.type = "button";
  element.addEventListener("click", action);
  return element;
}

function editorDialog({ heading, copy = "", titleValue, folders, selected = 0, confirm = "Salvar", action }) {
  document.querySelector("#dialog-title").textContent = heading;
  document.querySelector("#dialog-copy").textContent = copy;
  document.querySelector("#title-label").hidden = titleValue === undefined;
  document.querySelector("#folder-label").hidden = !folders;
  document.querySelector("#confirm").textContent = confirm;
  formTitle.required = titleValue !== undefined;
  formTitle.value = titleValue ?? "";
  formFolder.replaceChildren();
  if (folders) {
    for (const entry of folders) {
      const option = create("option", entry.title);
      option.value = entry.id;
      formFolder.append(option);
    }
    formFolder.value = selected;
  }
  submit = action;
  editor.showModal();
}

form.addEventListener("submit", event => {
  event.preventDefault();
  const action = submit;
  editor.close();
  action?.(formTitle.value, Number(formFolder.value));
});
document.querySelector("#cancel").addEventListener("click", () => editor.close());
for (const item of sectionButtons) {
  const page = item.dataset.page;
  item.addEventListener("click", () => { if (page !== section) send("show", { page }); });
}

function searchArea(label, draw) {
  const input = create("input", "", "search");
  input.type = "search";
  input.setAttribute("aria-label", label);
  input.placeholder = label;
  input.value = query;
  input.addEventListener("input", () => { query = input.value; draw(); });
  return input;
}

function matches(link) {
  const needle = query.trim().toLocaleLowerCase("pt-BR");
  return (link.title + " " + (link.url ?? "")).toLocaleLowerCase("pt-BR").includes(needle);
}

function row(name, subtitle, open, commands, favicon) {
  const item = create("div", "", "row");
  const details = create("div", "", "name");
  const title = open ? button(name, open) : create("strong", name);
  if (typeof favicon === "string" && /^[A-Za-z0-9+/=]{1,8192}$/.test(favicon)) {
    const icon = create("img", "", "site-icon");
    icon.alt = "";
    icon.draggable = false;
    icon.src = "data:image/png;base64," + favicon;
    title.prepend(icon);
  }
  details.append(title);
  if (subtitle) details.append(create("small", subtitle));
  const tools = create("div", "", "tools");
  for (const command of commands) tools.append(command);
  item.append(details, tools);
  return item;
}

function showBookmarks() {
  const select = id => { currentFolder = id; query = ""; showBookmarks(); };
  let dragged = null;
  let highlighted = null;
  const targets = new Map();
  const clearHighlight = () => {
    highlighted?.removeAttribute("data-drop");
    highlighted = null;
  };
  const dropTarget = (element, placementAt) => {
    targets.set(element, placementAt);
    element.classList.add("bookmark-target");
  };
  const folderButton = (title, id) => {
    const crumb = button(title, () => select(id));
    dropTarget(crumb, () => dragged.key.kind === "folder" && dragged.key.id === id
      ? null : { kind: "into", folder: id });
    return crumb;
  };
  const movable = (item, key) => {
    item.classList.add("bookmark-movable");
    let suppressClick = false;
    item.addEventListener("click", event => {
      if (suppressClick && event.detail !== 0) {
        event.preventDefault();
        event.stopPropagation();
      }
    }, true);
    item.addEventListener("pointerdown", start => {
      if (start.button !== 0 || start.pointerType !== "mouse" || dragged || start.target.closest(".tools")) return;
      suppressClick = false;
      const listeners = new AbortController();
      const options = { signal: listeners.signal };
      dragged = { key, row: item };
      const finish = () => {
        listeners.abort();
        clearHighlight();
        item.classList.remove("bookmark-dragging");
        if (item.hasPointerCapture(start.pointerId)) item.releasePointerCapture(start.pointerId);
        dragged = null;
      };
      const placementAt = event => {
        const target = document.elementFromPoint(event.clientX, event.clientY)?.closest(".bookmark-target");
        const placement = targets.get(target)?.(event);
        if (target !== highlighted || !placement) clearHighlight();
        if (placement) {
          highlighted = target;
          target.dataset.drop = placement.kind;
        }
        return placement;
      };
      document.addEventListener("pointermove", event => {
        if (event.pointerId !== start.pointerId) return;
        if (!item.isConnected || (event.buttons & 1) === 0) return finish();
        if (!suppressClick) {
          if (Math.hypot(event.clientX - start.clientX, event.clientY - start.clientY) < 6) return;
          suppressClick = true;
          item.setPointerCapture(event.pointerId);
          item.classList.add("bookmark-dragging");
        }
        placementAt(event);
      }, options);
      document.addEventListener("pointerup", event => {
        if (event.pointerId !== start.pointerId) return;
        const placement = suppressClick && item.isConnected && placementAt(event);
        finish();
        if (placement) send("moveBookmark", { item: key, placement });
      }, options);
      document.addEventListener("pointercancel", finish, options);
      item.addEventListener("lostpointercapture", finish, options);
      window.addEventListener("blur", finish, options);
      document.addEventListener("keydown", event => {
        if (event.key === "Escape") finish();
      }, options);
    });
    dropTarget(item, event => {
      if (dragged.row === item) return null;
      const bounds = item.getBoundingClientRect();
      const fraction = (event.clientY - bounds.top) / bounds.height;
      if (key.kind === "folder" && fraction >= 0.25 && fraction <= 0.75) {
        return { kind: "into", folder: key.id };
      }
      return { kind: fraction < 0.5 ? "before" : "after", item: key };
    });
  };
  if (currentFolder && !model.folders.some(folder => folder.id === currentFolder)) currentFolder = 0;
  const pathLabel = id => {
    const names = [];
    for (let i = 0; id && i < model.folders.length; i++) {
      const folder = model.folders.find(entry => entry.id === id);
      if (!folder) break;
      names.unshift(folder.title);
      id = folder.parent;
    }
    return names.join(" / ");
  };
  actions.replaceChildren(button("Nova pasta", () => editorDialog({
    heading: "Nova pasta", titleValue: "", confirm: "Criar pasta",
    action: title => send("addFolder", { parent: currentFolder, title }),
  }), "primary"));
  const crumbs = create("div", "", "breadcrumb");
  const segments = [];
  let cursor = currentFolder;
  while (cursor && segments.length < model.folders.length) {
    const folder = model.folders.find(entry => entry.id === cursor);
    if (!folder) break;
    segments.unshift(folder);
    cursor = folder.parent;
  }
  crumbs.append(folderButton("Todos os marcadores", 0));
  for (const folder of segments) crumbs.append(create("span", "›"), folderButton(folder.title, folder.id));
  const list = create("div", "", "panel");
  dropTarget(list, () => ({ kind: "into", folder: currentFolder }));
  const draw = () => {
    for (const item of list.children) targets.delete(item);
    list.replaceChildren();
    const filter = query.trim() !== "";
    const items = [];
    for (const folder of model.folders.filter(folder => filter ? matches(folder) : folder.parent === currentFolder)) {
      const item = row(folder.title, "Pasta", () => select(folder.id), [
        button("Renomear", () => editorDialog({ heading: "Renomear pasta", titleValue: folder.title,
          action: title => send("renameFolder", { id: folder.id, title }) })),
        button("Excluir", () => editorDialog({ heading: "Excluir pasta",
          copy: "Os marcadores e as subpastas serão movidos para a pasta superior.",
          confirm: "Excluir pasta", action: () => send("removeFolder", { id: folder.id }) }), "danger"),
      ]);
      if (!filter) movable(item, { kind: "folder", id: folder.id });
      items.push({ item, order: folder.order ?? 0 });
    }
    const folders = [{ id: 0, title: "Todos os marcadores" }, ...model.folders.map(folder => ({ id: folder.id, title: pathLabel(folder.id) }))];
    for (const link of model.bookmarks.filter(link => filter ? matches(link) : link.folder === currentFolder)) {
      const subtitle = filter && link.folder ? pathLabel(link.folder) + "  ·  " + link.url : link.url;
      const item = row(link.title || link.url, subtitle, () => send("open", { url: link.url }), [
        button("Editar ou mover", () => editorDialog({ heading: "Editar marcador",
          titleValue: link.title || link.url, folders, selected: link.folder,
          action: (title, folder) => send("editBookmark", { url: link.url, title, folder }) })),
        button("Excluir", () => send("removeBookmark", { url: link.url }), "danger"),
      ], link.favicon);
      if (!filter) movable(item, { kind: "link", url: link.url });
      items.push({ item, order: link.order ?? 0 });
    }
    items.sort((a, b) => a.order - b.order);
    for (const { item } of items) list.append(item);
    if (!list.children.length) list.append(create("p", filter ? "Nenhum marcador encontrado." : "Use a estrela da barra de endereços para salvar uma página.", "empty"));
  };
  views.replaceChildren(crumbs, searchArea("Pesquisar marcadores", draw), list);
  draw();
}

function showHistory() {
  const selected = new Set();
  const count = create("span", "", "selection-count");
  count.setAttribute("role", "status");
  const remove = visits => send("removeVisits", {
    visits: visits.map(link => ({ url: link.url, time: link.visited_at })),
  });
  let dayGroups = [];
  const setSelected = (link, checked) => {
    if (checked) selected.add(link);
    else selected.delete(link);
  };
  const clearSelection = button("Limpar seleção", () => {
    selected.clear();
    updateSelection();
  });
  const removeSelection = button("Excluir seleção", () => {
    const visits = [...selected];
    editorDialog({
      heading: "Excluir visitas",
      copy: `Excluir ${visits.length} ${visits.length === 1 ? "visita selecionada" : "visitas selecionadas"}?`,
      confirm: "Excluir", action: () => remove(visits),
    });
  }, "danger");
  const clearHistory = button("Limpar histórico", () => editorDialog({
    heading: "Limpar histórico", copy: "Excluir todas as visitas guardadas no histórico do Brisa?",
    confirm: "Limpar histórico", action: () => send("clearHistory"),
  }), "danger");
  actions.replaceChildren(count, clearSelection, removeSelection, clearHistory);

  function updateSelection() {
    for (const group of dayGroups) {
      let checked = 0;
      for (const [link, input] of group.items) {
        input.checked = selected.has(link);
        if (input.checked) checked++;
      }
      group.checkbox.checked = checked === group.items.length;
      group.checkbox.indeterminate = checked > 0 && !group.checkbox.checked;
    }
    const total = selected.size;
    count.textContent = total ? `${total} ${total === 1 ? "selecionada" : "selecionadas"}` : "";
    count.hidden = clearSelection.hidden = removeSelection.hidden = !total;
    clearHistory.hidden = total > 0;
    clearHistory.disabled = model.history.length === 0;
  }

  const content = create("div");
  const draw = () => {
    content.replaceChildren();
    dayGroups = [];
    const groups = new Map();
    for (const link of model.history.filter(matches)) {
      const date = link.visited_at ? new Date(link.visited_at) : null;
      const day = date ? date.toLocaleDateString("pt-BR", { dateStyle: "long" }) : "Sem data registrada";
      if (!groups.has(day)) groups.set(day, []);
      groups.get(day).push([link, date]);
    }
    for (const [day, visits] of groups) {
      const heading = create("div", "", "history-day");
      const dayCheckbox = create("input");
      dayCheckbox.type = "checkbox";
      dayCheckbox.setAttribute("aria-label", `Selecionar visitas de ${day}`);
      dayCheckbox.addEventListener("change", () => {
        for (const [link] of visits) setSelected(link, dayCheckbox.checked);
        updateSelection();
      });
      heading.append(dayCheckbox, create("h2", day));
      const list = create("div", "", "panel");
      const items = [];
      for (const [link, date] of visits) {
        const entry = row(link.title || link.url, link.url, () => send("open", { url: link.url }), [
          button("Remover", () => remove([link]), "danger"),
        ]);
        entry.classList.add("history-visit");
        const checkbox = create("input");
        checkbox.type = "checkbox";
        checkbox.setAttribute("aria-label", `Selecionar visita a ${link.title || link.url}`);
        checkbox.addEventListener("change", () => {
          setSelected(link, checkbox.checked);
          updateSelection();
        });
        entry.prepend(checkbox, create("span", date ? date.toLocaleTimeString("pt-BR", { timeStyle: "short" }) : "—", "date"));
        items.push([link, checkbox]);
        list.append(entry);
      }
      dayGroups.push({ checkbox: dayCheckbox, items });
      content.append(heading, list);
    }
    if (!content.children.length) content.append(create("p", "Nenhuma visita encontrada.", "subtle"));
    updateSelection();
  };
  views.replaceChildren(searchArea("Pesquisar histórico", () => {
    selected.clear();
    draw();
  }), content);
  draw();
}

function showSettings() {
  actions.replaceChildren();
  const groups = [
    ["Proteção e privacidade", [
      ["https_only", "Exigir HTTPS", "Usa conexão criptografada fora deste computador."],
      ["filter_network", "Bloquear anúncios e rastreadores", "Aplica EasyList e EasyPrivacy às requisições."],
      ["cross_site_popups", "Permitir pop-ups externos por clique", "Mantém aberturas automáticas bloqueadas. Com a opção desativada, use o escudo para permitir pop-ups externos de um site durante a sessão."],
    ]],
    ["Navegação", [
      ["show_bookmarks", "Mostrar barra de marcadores", "Acesso direto abaixo do endereço."],
      ["remember_history", "Guardar histórico", "Visitas em abas normais ficam no perfil local."],
      ["google_suggestions", "Sugestões do Google ao digitar", "Envia termos parciais ao Google em abas normais quando ele é o buscador selecionado."],
    ]],
  ];
  const categories = [];
  const appearance = create("div", "", "panel");
  const selectAppearance = (name, label, values) => {
    const row = create("label", "", "row setting");
    const select = create("select");
    select.name = name;
    for (const [value, text] of values) {
      const option = create("option", text);
      option.value = value;
      select.append(option);
    }
    select.value = model.preferences[name];
    select.addEventListener("change", () => send("setAppearance", {
      theme: appearance.querySelector("[name=theme]").value,
      accent: appearance.querySelector("[name=accent]").value,
    }));
    row.append(create("strong", label), select);
    appearance.append(row);
    return { element: row, text: label + " " + values.map(([, text]) => text).join(" ") };
  };
  categories.push({
    heading: create("h2", "Aparência"), panel: appearance,
    items: [
      selectAppearance("theme", "Modo de cor", [["light", "Claro"], ["dark", "Escuro"]]),
      selectAppearance("accent", "Cor de destaque", [["jade", "Jade"], ["blue", "Azul"], ["violet", "Violeta"]]),
    ],
  });
  for (const [heading, entries] of groups) {
    const panel = create("div", "", "panel");
    const items = [];
    for (const [key, caption, text] of entries) {
      const label = create("label", "", "row setting");
      const description = create("span");
      description.append(create("strong", caption), create("small", text));
      const input = create("input");
      input.type = "checkbox";
      input.name = key;
      input.checked = model.preferences[key];
      input.addEventListener("change", () => send("setPreference", { name: key, checked: input.checked }));
      label.append(description, input);
      panel.append(label);
      items.push({ element: label, text: caption + " " + text });
    }
    if (heading === "Navegação") {
      const row = create("label", "", "row setting");
      const select = create("select");
      select.name = "search_engine";
      for (const [value, title] of [["duck_duck_go", "DuckDuckGo"], ["google", "Google"], ["brave", "Brave Search"], ["bing", "Bing"]]) {
        const option = create("option", title);
        option.value = value;
        select.append(option);
      }
      select.value = model.preferences.search_engine;
      select.addEventListener("change", () => send("setSearchEngine", { engine: select.value }));
      row.append(create("strong", "Buscador da barra de endereço"), select);
      panel.prepend(row);
      items.push({ element: row, text: "Buscador da barra de endereço DuckDuckGo Google Brave Bing" });
    }
    categories.push({ heading: create("h2", heading), panel, items });
  }
  const languagePanel = create("div", "", "panel");
  const languages = [
    ["pt-BR", "Português (Brasil)"], ["en-US", "Inglês (Estados Unidos)"],
    ["es", "Espanhol"], ["fr", "Francês"], ["it", "Italiano"], ["de", "Alemão"],
  ];
  const languageItems = [
    ["website_language", "Idioma preferido dos sites", "Enviado aos sites e usado nos menus do motor após reiniciar o Brisa."],
    ["translation_language", "Traduzir para", "Idioma de destino ao traduzir uma página."],
  ].map(([key, caption, hint]) => {
    const row = create("label", "", "row setting");
    const description = create("span");
    description.append(create("strong", caption), create("small", hint));
    const select = create("select");
    select.name = key;
    for (const [value, title] of languages) {
      const option = create("option", title);
      option.value = value;
      select.append(option);
    }
    select.value = model.preferences[key];
    select.addEventListener("change", () => send("setLanguages", {
      website: languagePanel.querySelector("[name=website_language]").value,
      translation: languagePanel.querySelector("[name=translation_language]").value,
    }));
    row.append(description, select);
    languagePanel.append(row);
    return { element: row, text: [caption, hint, ...languages.map(([, title]) => title)].join(" ") };
  });
  categories.push({ heading: create("h2", "Idiomas"), panel: languagePanel, items: languageItems });
  const downloads = create("div", "", "panel");
  const downloadFolder = create("div", "", "row setting");
  const folderDescription = create("span");
  folderDescription.append(
    create("strong", "Pasta de downloads"),
    create("small", model.download_folder || "Pasta padrão do Windows", "download-folder"),
  );
  downloadFolder.append(folderDescription, button("Alterar pasta", () => send("chooseDownloadFolder")));
  downloads.append(downloadFolder);
  categories.push({
    heading: create("h2", "Downloads"), panel: downloads,
    items: [{ element: downloadFolder, text: "Pasta de downloads Local de salvamento Alterar pasta" }],
  });
  const clearCache = button("Esvaziar cache", () => send("clearCache"));
  const clear = button("Limpar cookies e dados de sites", () => send("clearSites"), "danger");
  const dataTools = create("div", "", "tools");
  dataTools.append(clearCache, clear);
  categories.push({
    heading: create("h2", "Dados de sites"), panel: dataTools,
    items: [
      { element: clearCache, text: clearCache.textContent },
      { element: clear, text: clear.textContent },
    ],
  });
  const empty = create("p", "Nenhuma configuração encontrada.", "subtle");
  empty.setAttribute("role", "status");
  const normalize = text => text.toLocaleLowerCase("pt-BR").normalize("NFD").replace(/\p{M}/gu, "");
  const draw = () => {
    const needle = normalize(query.trim());
    let found = false;
    for (const category of categories) {
      const headingMatches = normalize(category.heading.textContent).includes(needle);
      let visible = false;
      for (const item of category.items) {
        item.element.hidden = !headingMatches && !normalize(item.text).includes(needle);
        visible ||= !item.element.hidden;
      }
      category.heading.hidden = category.panel.hidden = !visible;
      found ||= visible;
    }
    empty.hidden = found;
  };
  views.replaceChildren(
    searchArea("Pesquisar configurações", draw),
    ...categories.flatMap(({ heading, panel }) => [heading, panel]),
    empty,
  );
  draw();
}

function showProtection() {
  actions.replaceChildren(button("Atualizar", () => send("ready")));
  const host = create("h2", model.host || "Proteção da página");
  const description = create("p", model.connection || "Abra os detalhes pelo escudo da aba que deseja verificar.", "subtle");
  const requests = model.requests ?? 0;
  const popups = model.popups ?? 0;
  const summary = create("p", `${requests} ${requests === 1 ? "requisição bloqueada" : "requisições bloqueadas"} · ${popups} ${popups === 1 ? "pop-up bloqueado" : "pop-ups bloqueados"}`, "selection-count");
  const list = create("div", "", "panel");
  const draw = () => {
    list.replaceChildren();
    const entries = model.entries.filter(entry => matches({ title: entry.target, url: entry.kind }));
    for (const entry of entries) {
      list.append(row(entry.target, entry.kind, null, [create("span", `×${entry.count}`, "subtle")]));
    }
    if (!entries.length) list.append(create("p", "Nenhum bloqueio encontrado.", "empty"));
  };
  views.replaceChildren(host, description, summary, searchArea("Pesquisar bloqueios", draw), list);
  if (model.unlisted) {
    views.append(create("p", `${model.unlisted} bloqueios adicionais contabilizados.`, "subtle"));
  }
  draw();
}

bridge.addEventListener("message", event => {
  const value = event.data;
  if (!value || !value.data || !Object.hasOwn(titles, value.section)) return;
  const changed = section !== value.section;
  if (changed) {
    if (editor.open) editor.close();
    submit = null;
    section = value.section;
    query = "";
    const title = titles[section];
    document.querySelector("#page-title").textContent = title;
    document.title = title + " | Brisa";
    for (const item of sectionButtons) {
      item.hidden = Boolean(value.private) && item.dataset.page !== section;
      if (item.dataset.page === section) item.setAttribute("aria-current", "page");
      else item.removeAttribute("aria-current");
    }
  }
  feedback.textContent = value.message || "";
  model = value.data;
  document.documentElement.dataset.theme = value.theme;
  document.documentElement.dataset.accent = value.accent;
  if (section === "marcadores") showBookmarks();
  if (section === "historico") showHistory();
  if (section === "protecao") showProtection();
  if (section === "configuracoes") {
    if (changed || !views.querySelector(".search")) {
      showSettings();
    } else {
      for (const field of views.querySelectorAll(".setting input, .setting select")) {
        if (field.type === "checkbox") field.checked = model.preferences[field.name];
        else field.value = model.preferences[field.name];
      }
      views.querySelector(".download-folder").textContent = model.download_folder || "Pasta padrão do Windows";
    }
  }
  if (changed) window.scrollTo(0, 0);
});
send("ready");
