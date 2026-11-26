# Publicar o Brisa 1.0.5

Use um repositório **público e exclusivo do Brisa**. O GitHub Pages oferece hospedagem de páginas estáticas em repositórios públicos no plano gratuito. [GitHub Pages](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)

1. No GitHub, clique em **New repository**. Use o nome `brisa-browser`, escolha **Public**, marque **Add a README file** e crie o repositório.
2. Na aba **Code**, use **Add file › Upload files**. Abra o seletor de arquivos e entre na pasta `docs` do kit ou do projeto. Clique em um arquivo da lista e pressione **Ctrl+A** para selecionar `.nojekyll`, `brisa.ico`, `brisa.png`, `download.js`, `index.html` e `styles.css`. Clique em **Abrir** e em **Commit changes**. Confira se `index.html` aparece na raiz do repositório, ao lado do README. [Upload de arquivos no GitHub](https://docs.github.com/en/repositories/working-with-files/managing-files/adding-a-file-to-a-repository)
3. Em **Releases › Draft a new release**, crie a tag `v1.0.5`. Anexe `Brisa-1.0.5-Windows-x64.exe` e `SHA256SUMS.txt`, da pasta `release` do kit ou da pasta `target/distribution` do projeto. Mantenha os nomes dos arquivos e clique em **Publish release**. [Links de releases](https://docs.github.com/en/repositories/releasing-projects-on-github/linking-to-releases)
4. Em **Settings › Pages**, escolha **Deploy from a branch**, selecione `main` e `/(root)` e clique em **Save**. Use `/docs` somente se `index.html` estiver dentro de uma pasta chamada `docs` no GitHub.
5. Depois do primeiro deploy, clique em **Visit site** na página **Settings › Pages**. Se o repositório se chamar `brisa-browser`, o endereço terá a forma `https://SEU-USUARIO.github.io/brisa-browser/`. Teste o botão **Baixar para Windows**.

O kit de publicação contém os arquivos do site em `docs` e os anexos da release em `release`. O botão da página usa os arquivos da release no mesmo repositório.

Para o aviso de atualização no Brisa funcionar nas próximas versões, publique uma release estável com tag `vX.Y.Z` e um instalador chamado `Brisa-X.Y.Z-Windows-x64.exe`. Marque a release como recente. O Brisa consulta o GitHub ao iniciar, a cada seis horas e no menu **⋯ › Verificar atualizações**. A partir da versão 1.0.4, o atalho também solicita uma consulta quando a janela já está aberta.

Em uma página de projeto do GitHub Pages, `download.js` aponta os dois links para os arquivos da release no mesmo repositório. Se você abrir o site do kit extraído no seu computador, ele usa a pasta `release` do kit.

Use este texto como descrição da release.

> O Brisa 1.0.5 para Windows x64 seleciona o endereço inteiro com um duplo clique na barra de endereços.
