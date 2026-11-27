"use strict";

if (location.protocol === "file:") {
  const folder = document.body.dataset.localDownloadRoot;
  for (const link of document.querySelectorAll("a[data-local-file]")) {
    link.href = folder + encodeURIComponent(link.dataset.localFile);
  }
}
