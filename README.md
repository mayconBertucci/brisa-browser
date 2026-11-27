# Brisa 1.0.6

O Brisa é um navegador gratuito para Windows x64. A interface e a lógica do aplicativo são escritas em Rust. O Microsoft Edge WebView2 renderiza as páginas e mantém o motor atualizado pelo Windows.

## Baixar e instalar

1. Baixe o [instalador mais recente do Brisa para Windows](https://github.com/mayconBertucci/brisa-browser/releases/latest/download/Brisa-Windows-x64.exe).
2. Feche as versões anteriores do Brisa e abra o instalador. Escolha português, espanhol ou inglês e selecione o tipo de instalação.
3. Abra **Brisa** pelo menu Iniciar. O Google abre por padrão. Se você abrir o Brisa novamente com o mesmo perfil, a versão atual traz a janela existente à frente ou chama sua atenção na barra de tarefas.

Funciona em Windows 10 e 11 x64. **Somente para mim** instala em `%LOCALAPPDATA%\Programs\Brisa` sem pedir administrador. **Para todos os usuários** instala em `%ProgramFiles%\Brisa` e pede autorização do Windows. Em ambos os casos, o Brisa roda com as permissões da conta e usa o perfil de cada usuário em `%LOCALAPPDATA%\BrisaBrowser\WebView2`.

Quando o WebView2 Evergreen está ausente, o instalador baixa o runtime da Microsoft. O desinstalador está em **Configurações do Windows › Aplicativos**. Para trocar de instalação por usuário para instalação para todos, desinstale a cópia por usuário primeiro para ter um único atalho no menu Iniciar.

Se aparecer **Este perfil do Brisa já está aberto**, localize as janelas de versões anteriores pelo menu da barra de tarefas e feche-as antes de abrir a versão nova. O arquivo de bloqueio protege os favoritos, as configurações e os dados da sessão contra atualizações de duas versões ao mesmo tempo.

O instalador da versão 1.0.6 é distribuído sem assinatura Authenticode. O SmartScreen pode sinalizar um aplicativo cuja reputação ainda é desconhecida. Compare o resultado do comando abaixo com a soma SHA-256 publicada na release. [Soma SHA-256 da versão mais recente](https://github.com/mayconBertucci/brisa-browser/releases/latest/download/SHA256SUMS.txt) · [Reputação no SmartScreen](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)

```powershell
Get-FileHash -LiteralPath .\Brisa-Windows-x64.exe -Algorithm SHA256
```

## O que você ganha

- **Menos interrupções.** O motor de filtragem usa EasyList e EasyPrivacy para bloquear anúncios de rede e rastreadores. Também barra pop-ups automáticos e destinos reconhecidos como anúncios.
- **Controle dos bloqueios.** Clique no escudo ou no contador da barra de favoritos para ver o endereço e o tipo de cada bloqueio. Você pode pausar o filtro para o site atual durante a sessão.
- **Proteção em conexões.** HTTPS é exigido por padrão fora de loopback. O Brisa também verifica endereços IP privados, locais e link-local em requisições de páginas públicas. O WebView2 cuida dos certificados.
- **Sessões funcionais.** O filtro atua sobre requisições que correspondem às regras. Cookies de requisições permitidas continuam disponíveis. O teste local confirmou login em outra aba, sessão após reiniciar e preservação da sessão ao limpar apenas o cache.
- **Uma interface enxuta.** Abas fixadas, favoritos, histórico, bloqueios e configurações fazem parte do próprio navegador. A estante guarda o endereço e libera a aba até você voltar.

### RAM e espaço no disco

| Navegador | RAM privada mediana | Perfil no disco |
| --- | ---: | ---: |
| **Brisa 1.0.0** | **76,31 MiB** | **5,32 MiB** |
| Brave | 127,43 MiB | 72,51 MiB |
| Chrome | 209,09 MiB | 18,06 MiB |

Medição no mesmo Windows, com uma página local de 120 artigos, um perfil novo por execução e três execuções por navegador. A RAM inclui a árvore de processos, incluindo o WebView2 no Brisa. O espaço em disco mede o conteúdo do perfil de teste. Mais abas, vídeos e aplicações web podem mudar o resultado. [Como reproduzir](#desenvolver-e-verificar)

## Primeiros passos

| Recurso | Como usar |
| --- | --- |
| Barra de endereços | **Ctrl+L** foca o campo. Um duplo clique ou **Ctrl+A** seleciona o endereço inteiro. Setas e **Enter** escolhem uma sugestão. Os marcadores e o histórico fornecem sugestões locais. Com o Google selecionado, o Brisa também pode enviar termos parciais para obter sugestões em abas normais. Desative essa opção em **Configurações › Navegação**. |
| Abas e página inicial | **Ctrl+T** abre o Google com o campo de endereço pronto para digitar. Um ícone animado aparece quando a página demora a carregar. Arraste uma aba para mudar sua posição. Clique com o botão direito para fixar, desafixar ou fechar. As abas normais fixadas voltam depois de reiniciar. |
| Favoritos | Clique na estrela ou use **Ctrl+D**. Arraste para uma borda de outro item para reordenar e solte no centro de uma pasta para mover para dentro. **Ctrl+Shift+O** abre o gerenciador. Use o caminho acima da lista para mover de volta a uma pasta superior. |
| Histórico | Use **Ctrl+H** para pesquisar visitas e excluir itens selecionados ou um dia de cada vez. |
| Aparência | Em **⋯ › Configurações**, escolha tema claro ou escuro e a cor Jade, Azul ou Violeta. |
| Zoom | Use **Ctrl + roda do mouse**. O percentual aparece ao lado do endereço. **Ctrl+0** restaura 100%. |
| Privacidade | **Ctrl+Shift+N** abre uma aba privada. Em **Dados de sites**, **Esvaziar cache** preserva os cookies. |
| Tradução | Em **⋯ › Ferramentas**, use **Traduzir página** ou **Mostrar texto original**. Defina os idiomas preferidos em **Configurações › Idiomas**. |
| DevTools | Pressione **F12** para inspecionar a aba. Os ícones do Brisa permitem encaixar à esquerda, embaixo ou à direita, ou abrir uma janela separada. |
| Downloads | Use o botão **Downloads** ou **Ctrl+J** para ver os downloads recentes desta sessão e abrir arquivos concluídos. Clique fora do painel para fechá-lo. Escolha **Alterar pasta…** no painel ou **Configurações › Downloads** para definir onde salvar os próximos arquivos. |
| Atualizações | No perfil padrão, o Brisa consulta novas versões ao abrir, quando o atalho reativa a janela e a cada seis horas. No menu **⋯ › Atualização disponível**, confirme em **Atualizar**. O Brisa baixa o instalador oficial, verifica seu SHA-256 e pede ao Windows para iniciá-lo. O instalador atualiza a instalação existente e reabre o navegador. O Windows pode pedir sua confirmação. |
| Estante | Use **Guardar aba na estante**. O Brisa libera a aba e carrega a página novamente quando você a reabre. |

### Tradução local

Para usar o tradutor, instale o [Node.js](https://nodejs.org/pt-br/download) no diretório padrão do Windows, `C:\Program Files\nodejs`. Na primeira tradução, o Brisa baixa o modelo Bergamot e verifica seu hash. O texto da página é processado no computador. Os idiomas disponíveis incluem português, inglês, espanhol, francês, italiano e alemão. A tradução cobre texto visível no documento principal, dentro do limite por página. [Bergamot](https://github.com/browsermt/bergamot-translator)

## Segurança e compatibilidade

A [revisão de segurança](SEGURANCA.md) descreve as verificações de origem, o filtro de rede local e a auditoria das dependências. A lista de bloqueios fica na memória durante a navegação e omite parâmetros e fragmentos dos endereços.

Passe o mouse sobre o escudo para ler o estado da conexão e saber se o filtro está pausado. Clique para abrir **Ver bloqueios e conexão**. Uma navegação convertida em download conserva o estado da página anterior. A consulta de atualizações envia uma requisição HTTPS ao endpoint público de releases do projeto, com a versão instalada no cabeçalho do cliente.

Sites podem detectar a ausência de recursos que esperam carregar. Anúncios entregues junto ao conteúdo podem continuar visíveis. Alguns provedores de login aplicam regras próprias a WebViews. O [inventário de paridade](PARIDADE.md) acompanha recursos como extensões, sincronização, gerenciamento de senhas e cobertura de anúncios na página.

## Desenvolver e verificar

Em um Windows x64 com Rust, ferramentas de compilação C/C++ e Node.js, execute na pasta do projeto. O script usa a toolchain local em `.toolchain` quando ela está disponível. Acrescente `-Offline` quando todos os insumos estiverem no cache.

```powershell
powershell.exe -NoProfile -File .\build.ps1 -Test -Translation
.\target\release\brisa.exe
node .\verify-browser.mjs .\target\release\brisa.exe
python .\benchmark.py --brisa .\target\release\brisa.exe --runs 3 --output .\evidence\benchmark-v1.0.json
```

Para gerar o instalador, use [Inno Setup 6](https://jrsoftware.org/inno-setup/downloads) e informe o caminho do compilador quando ele estiver fora do `PATH` ou da `.toolchain`.

```powershell
powershell.exe -NoProfile -File .\package.ps1 -Compiler 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
```

O resultado fica em `target\distribution`. As [licenças e atribuições](installer/TERCEIROS.md) acompanham o aplicativo.

Para assinar o aplicativo e o instalador, adicione `-CodeSigningThumbprint` com a impressão digital de um certificado de assinatura de código disponível na sua conta e indique o `signtool.exe` do SDK do Windows por `-SignTool` quando ele estiver fora do PATH. O script verifica as assinaturas antes de publicar o SHA-256.

## Página de lançamento

Abra [a página local do Brisa](docs/index.html). O site é estático e pode ser publicado no GitHub Pages de um repositório público dedicado ao Brisa. Use o [kit de publicação](target/distribution/Brisa-1.0.6-GitHub-Pages.zip) e siga o [guia de publicação](installer/PUBLICAR.md).
