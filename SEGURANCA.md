# Revisão de segurança do Brisa

## Correções

| Área | Comportamento revisado |
| --- | --- |
| Navegação local | O Brisa verifica o destino de navegações, pop-ups e requisições interceptadas. Para páginas públicas, isso bloqueia loopback, intervalos IPv4 privados e link-local, IPv6 local e IPv4 local representado como IPv6. A navegação pela barra continua disponível para um endereço local explícito de acordo com a regra de HTTPS. [RFC 6761](https://www.rfc-editor.org/rfc/rfc6761.html#section-6.3) |
| Perfil local | A gravação confere o mesmo limite de tamanho usado na leitura. A substituição do perfil só ocorre depois que o arquivo temporário válido foi gravado. |
| Arrastar favoritos | O gerenciador aceita arrastes iniciados com o ponteiro na própria página interna. O código Rust confirma a existência dos itens e impede que uma pasta seja movida para dentro de si mesma ou de suas subpastas. |
| Privacidade | Fechar a última aba privada libera o último controlador InPrivate e remove as exceções privadas da sessão. Quando ela é a única aba da janela, o Brisa volta à página inicial interna. |
| Regras adicionais | A opção `--rules` lê até 8 MiB de texto UTF-8 e informa o erro ao abrir um arquivo maior. |
| Cache de rede | O comando de limpeza é fixo e só pode ser solicitado pela página interna de configurações. A ação usa a limpeza de cache do motor e preserva os cookies. [Protocolo de cache do Chromium](https://chromedevtools.github.io/devtools-protocol/tot/Network/#method-clearBrowserCache) |

## Limites e dependências

O filtro de transporte, o bloqueio de pop-ups e a verificação de origem na ponte de páginas internas foram revisados no código Rust. A proteção de rede local examina os endereços escritos na URL e os nomes especiais sob `.localhost`. A resolução DNS e a cobertura dos eventos dependem do motor. O controle de navegação do WebView2 pode começar uma requisição GET durante a decisão do aplicativo. [Segurança em WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/security) · [Evento de início de navegação](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2navigationstartingeventargs)

A revisão do Brisa 0.10.1 consultou o RustSec local por meio do `cargo-audit` e o OSV em 26 de novembro de 2026. A consulta ao OSV retornou zero avisos para 69 versões de crates e uma dependência npm fixadas nos arquivos de lock. A verificação do motor Chromium e das proteções do Windows tem escopo próprio. [RustSec](https://rustsec.org/) · [OSV](https://osv.dev/)

## Extensões

A ativação e o gerenciamento de extensões ficam indisponíveis pela biblioteca `windows-webview 0.100.0` usada pelo Brisa. A API do motor aceita extensões locais descompactadas por perfil quando habilitada no ambiente. Uma implementação precisa de controles de instalação, habilitação e remoção, isolamento das páginas internas e pontos de entrada para as interfaces das extensões. A API de instalação por pasta oferece uma base para esse trabalho. [Extensões por perfil no WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2profile7) · [Opção do ambiente](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2environmentoptions6)
