# Auditoria de usabilidade do Brisa 0.7

25 de novembro de 2026 · Windows · versão `0.7.0` em release

A auditoria combina testes Rust, integração com o WebView2, uso da janela com mouse e teclado e inspeção visual. Usei perfis descartáveis, páginas e contas fictícias e uma página pública de teste para o download e a tradução. A interface do navegador foi avaliada em relação aos critérios de interação, contraste, formulários e recuperação de erros da skill **Web Design Guidelines** da Vercel. O catálogo mostrava cerca de 906 mil instalações. [Skill e documentação](https://www.skills.sh/vercel-labs/agent-skills/web-design-guidelines) · [Critério WCAG 2.2 de contraste](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)

## Cobertura funcional

| Área | Resultado observado |
| --- | --- |
| Janela e abas | Maximizar e restaurar, abrir, trocar, fechar, reabrir e escolher uma aba na lista funcionaram. Guardar uma aba na estante funcionou. A reabertura funcionou com um site ativo e falhou a partir do centro interno. |
| Navegação e busca | Google abriu em nova aba e foi confirmado como buscador padrão. Voltar e avançar funcionaram. Uma sugestão de marcador apareceu para uma consulta sem acento e abriu o endereço correspondente. Os quatro buscadores disponíveis puderam ser selecionados e salvos. |
| Marcadores | Salvar por atalho, editar pelo botão direito, alternar a barra e criar pasta no espaço livre da barra funcionaram. No gerenciador, criar e renomear pasta, editar, mover, abrir e excluir marcador funcionaram. Escape fechou o formulário. A busca sem acento falhou e a exclusão de marcador foi imediata. |
| Histórico, configurações e páginas internas | A integração removeu duas visitas selecionadas de cinco. As quatro seções do centro trocaram de conteúdo no mesmo documento. As preferências de aparência, barra e histórico foram gravadas. Cliquei no contador de bloqueios e cheguei à seção correta pela janela. |
| Aparência e teclado | Conferi modo claro e escuro com jade, azul e violeta. Os oito controles percorridos por Tab na página de configurações exibiram foco visível. A barra lateral do centro condensou a 480 px CSS sem rolagem horizontal. A 320 px, a navegação interna exigiu rolagem, embora o conteúdo principal coubesse. |
| Anúncios, rastreadores e pop-ups | O cenário nativo mostrou dois bloqueios de rede e um pop-up e listou os destinos de teste `/audit-ad.svg`, `/audit-tracker.js` e `/popup-teste`. A busca pelo rastreador filtrou a lista. A integração confirmou que o servidor não recebeu as requisições bloqueadas e que um script funcional de outro site carregou. |
| Sessões e privacidade | O cookie fictício funcionou em outra aba, após reiniciar e após limpar o cache. Sair encerrou a sessão. Dois formatos de pop-up de autenticação fictícia devolveram o resultado à aba de origem e preservaram o cookie do provedor. Em navegação privada, a sessão regular ficou ausente. Fechar a última aba privada descartou a sessão privada. A visita privada identificada não apareceu no histórico salvo. |
| Inspetor, zoom e tradução | F12 abriu a janela DevTools para o site local. O zoom variou de 100% para 110% e voltou a 100%. A tradução recusou HTTP local com uma explicação. Para o endereço HTTPS público, pediu confirmação e abriu uma aba do Google Tradutor com destino em português. O teste de tradução cobriu o encaminhamento da URL. |
| Downloads | O arquivo HTTP de teste foi recusado e ficou ausente do disco. O arquivo de amostra via HTTPS pediu confirmação, foi salvo na pasta de teste e apareceu como concluído em **Downloads da sessão**. Ctrl+J ficou sem resposta visível na página do teste. [Página pública usada como amostra](https://wpt.live/common/blank.html) |

## Problemas encontrados

| Prioridade | Problema e como reproduzir |
| --- | --- |
| Alta | **Estante falha com uma aba interna ativa.** Guarde um site em Ferramentas, vá a Bloqueios, escolha **Abrir estante** e clique no site. O menu fecha e o centro permanece na tela, sem aviso. A mesma opção reabriu o site com uma aba comum ativa. O caminho da estante chama `navigate` diretamente. [browser.rs](src/browser.rs#L1484) |
| Média | **Exclusão acidental de marcador.** No gerenciador, **Excluir** em uma linha remove o marcador no primeiro clique, sem confirmação nem opção de desfazer. Excluir uma pasta ou uma seleção do histórico já pede confirmação. [center.js](src/center.js#L132) |
| Média | **Contraste da dica de busca.** A proporção medida foi **4,26:1** no tema claro e **3,59:1** no escuro, abaixo dos **4,5:1** exigidos para texto comum. O menor contraste dos textos principais, secundários, destaques e botões medidos foi **5,88:1**. [center.css](src/center.css#L48) · [WCAG 2.2](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) |
| Média | **Busca inconsistente em português.** Salve “Página de referência” e procure “referencia” no gerenciador. O resultado some. “referência” encontra o marcador. A barra de endereços encontrou o marcador no teste sem acento. [center.js](src/center.js#L80) |
| Média | **Downloads difíceis de reencontrar pelo teclado.** Ctrl+J não abriu a lista durante o teste. A ação no menu abriu o arquivo concluído em uma caixa modal. [browser.rs](src/browser.rs#L1741) |
| Média | **O visual perde consistência em alguns pontos.** O menu principal tem estilo próprio. A lista de abas e a estante usam menus nativos do Windows, e os diálogos de downloads e tradução misturam texto em português com botões “Sí”, “No” e “Aceptar” neste Windows em espanhol. Ajustar o zoom fecha o submenu e exige reabri-lo para conferir o percentual. |
| Baixa | **Navegação do centro estreita.** A 320 px CSS, a faixa das seções precisa de 119 a 135 px de rolagem horizontal. Algumas opções ficam fora do primeiro campo visível. |

## Alcance da auditoria

- Os **10 testes Rust** passaram em release. O Clippy passou com `-D warnings`. A integração passou no WebView2 com os cenários acima e registrou as falhas de acessibilidade e interação separadamente.
- A troca de seção nas páginas internas manteve o mesmo documento no cenário de integração. A verificação não mediu quadros de vídeo ou tempos de composição. Ainda falta fechar o diagnóstico do piscar entre sites, abas e superfícies nativas.
- A exceção temporária por origem e a permissão de pop-up foram verificadas por testes Rust. Ficaram fora da verificação de ponta a ponta a limpeza completa de dados dos sites, os provedores de login reais, o texto traduzido, leitores de tela, DPI diferentes e sessões longas.
- Extensões, sincronização, grupos de abas e gerenciador persistente de downloads são exemplos de recursos ainda sem implementação no inventário da versão. [Inventário de paridade](PARIDADE.md)
- O executável testado é o da 0.7. Foram adicionadas verificações de auditoria ao script de integração.

## Evidências e reprodução

- [Resultados automatizados](evidence/qa-usabilidade-v0.7.json)
- [Configurações em modo claro](evidence/usabilidade-v0.7/configuracoes-light.png) · [Configurações em modo escuro](evidence/usabilidade-v0.7/configuracoes-dark.png) · [Configurações a 320 px](evidence/usabilidade-v0.7/pagina-estreita-320.png)
- [Gerenciador de marcadores](evidence/usabilidade-v0.7/marcadores-escuro.png) · [Histórico](evidence/history-audit-v0.7.png)

Do diretório do projeto, com Node.js e o executável release disponíveis:

```powershell
node ./verify-browser.mjs ./target/release/brisa-07.exe ./evidence/history-audit-v0.7.png ./evidence/usabilidade-v0.7
```
