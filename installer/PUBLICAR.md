# Publicar o Brisa 1.0.6

Use um repositório **público e exclusivo do Brisa**. O GitHub Pages oferece hospedagem de páginas estáticas em repositórios públicos no plano gratuito. [GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)

1. No GitHub, clique em **New repository**. Use o nome `brisa-browser`, escolha **Public**, marque **Add a README file** e crie o repositório.
2. Na aba **Code**, use **Add file › Upload files**. Envie os arquivos de `docs`, incluindo `index.html`, `es.html` e `en.html`, para a raiz do repositório. [Upload de arquivos no GitHub](https://docs.github.com/en/repositories/working-with-files/managing-files/adding-a-file-to-a-repository)
3. Em **Releases › Draft a new release**, crie a tag `v1.0.6` e anexe `Brisa-1.0.6-Windows-x64.exe`, `Brisa-Windows-x64.exe` e `SHA256SUMS.txt`. Os três arquivos estão na pasta `release` do kit ou em `target/distribution`. Marque a release como a mais recente e publique. [Links de releases](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)
4. Em **Settings › Pages**, escolha **Deploy from a branch**, selecione `main` e `/(root)` e clique em **Save**. Use `/docs` somente se `index.html` estiver dentro de uma pasta chamada `docs` no GitHub.
5. Depois do primeiro deploy, clique em **Visit site** na página **Settings › Pages**. Se o repositório se chamar `brisa-browser`, o endereço terá a forma `https://SEU-USUARIO.github.io/brisa-browser/`. Teste o botão **Baixar para Windows**.

O kit de publicação contém os arquivos do site em `docs` e os anexos da release em `release`. O botão da página usa os arquivos da release no mesmo repositório.

Nas próximas versões, publique a tag estável `vX.Y.Z` com o instalador versionado, o mesmo instalador com o nome fixo `Brisa-Windows-x64.exe` e o `SHA256SUMS.txt` correspondente. Marque a release como a mais recente. O botão do site passa a baixar o instalador da nova release automaticamente. O atualizador do Brisa verifica o instalador versionado com o SHA-256 fornecido pela API do GitHub. [Metadados dos anexos de releases](https://docs.github.com/en/rest/releases/assets)

Ao abrir o site a partir do kit extraído, os downloads usam a pasta `release` do próprio kit.

Use este texto como descrição da release.

> O Brisa 1.0.6 para Windows x64 confirma e instala atualizações pelo navegador. É possível alterar a pasta de downloads no painel ou nas configurações. O painel fecha ao clicar fora. O site e o instalador estão disponíveis em português, espanhol e inglês.
