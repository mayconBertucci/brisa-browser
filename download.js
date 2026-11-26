"use strict";

const pages = /^([a-z0-9-]+)\.github\.io$/i.exec(location.hostname);
const project = location.pathname.split("/")[1];
const version = document.querySelector('meta[name="brisa-version"]').content;
if (pages && /^[a-z0-9_.-]+$/i.test(project)) {
  const release = `https://github.com/${pages[1]}/${project}/releases/download/v${version}/`;
  for (const link of document.querySelectorAll("a[data-release]")) {
    link.href = release + encodeURIComponent(link.dataset.release);
  }
}
