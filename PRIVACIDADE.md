# Privacidade no Brisa

## Dados no computador

O Brisa guarda configurações, marcadores, abas fixadas e histórico no perfil local do Windows. O motor WebView2 também usa esse perfil para armazenar dados de sites, como cookies e cache. Você pode apagar visitas no **Histórico** e desativar **Guardar histórico** em **Configurações › Navegação**.

As abas privadas usam uma sessão InPrivate do WebView2. As visitas dessas abas ficam fora do histórico do Brisa. Os arquivos baixados continuam na pasta de destino depois que você fecha a aba.

## Conexões com serviços

- **Páginas e pesquisas.** Ao navegar, o navegador comunica-se com os sites visitados. Ao pesquisar, envia o termo ao buscador selecionado. Com o Google selecionado, **Sugestões do Google ao digitar** envia termos parciais ao Google para obter sugestões em abas normais. Você pode desativar essa opção em **Configurações › Navegação**. [Política de Privacidade do Google](https://policies.google.com/privacy?hl=pt-BR)
- **Atualizações.** No perfil padrão, o Brisa consulta as releases públicas do GitHub ao abrir e periodicamente. A requisição inclui a versão do Brisa no identificador do cliente. [Política de Privacidade do GitHub](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)
- **Tradução.** Na primeira tradução para um idioma sem modelo no computador, o Brisa baixa o modelo correspondente. O processamento do texto da página ocorre localmente.
- **Motor e proteção.** O Brisa usa o Microsoft Edge WebView2 e mantém o Microsoft Defender SmartScreen habilitado por padrão. O motor pode enviar à Microsoft informações de diagnóstico e de reputação de sites ou downloads. O Windows oferece controles para os dados opcionais de diagnóstico. [Dados e privacidade no WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy) · [Política de Privacidade da Microsoft](https://privacy.microsoft.com/pt-br/privacystatement)
