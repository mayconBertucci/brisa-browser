(() => {
  const language = (document.documentElement.lang ||
    document.querySelector('meta[http-equiv="content-language" i]')?.content || "")
    .trim().toLowerCase().split(/[-_,]/u)[0];
  const nodes = [];
  const originals = [];
  const skipped = "script,style,textarea,select,input,pre,code,svg,math,[contenteditable],[translate=no],.notranslate,[hidden],[aria-hidden=true]";
  const walker = document.createTreeWalker(document.body ?? document.documentElement, NodeFilter.SHOW_TEXT);
  let used = 0;
  let scanned = 0;
  for (let node = walker.nextNode(); node && scanned < 16_000; node = walker.nextNode()) {
    scanned++;
    const parent = node.parentElement;
    const original = node.data;
    if (!parent || original.length > 2_400 || !/\p{Letter}/u.test(original) ||
        parent.closest(skipped) || parent.getClientRects().length === 0) continue;
    if (nodes.length >= 450 || used + original.length > 30_000) break;
    nodes.push(node);
    originals.push(original);
    used += original.length;
  }
  let applied = null;
  globalThis.brisaPageTranslation = {
    apply(translations) {
      if (!Array.isArray(translations) || translations.length !== originals.length) return false;
      applied = translations;
      nodes.forEach((node, i) => {
        if (node.isConnected && node.data === originals[i]) node.data = translations[i];
      });
      return true;
    },
    restore() {
      if (!applied) return;
      nodes.forEach((node, i) => {
        if (node.isConnected && node.data === applied[i]) node.data = originals[i];
      });
      applied = null;
    },
  };
  return { language, text: originals };
})();
