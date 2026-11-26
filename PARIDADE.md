# Paridade do Brisa no Windows

Referências locais. Chrome 158.0.8127.89 e Brave 158.1.100.59. As capturas do usuário mostram abas, menu, histórico e marcadores do Chrome. Os relatórios do REA em `evidence/` registram recursos e importações dos executáveis. A inspeção em execução, com perfis temporários, registrou os controles expostos em histórico, marcadores, downloads e configurações em `evidence/reference-pages-v158.json`.

| Área | Disponível no Brisa 1.0 | Trabalho pendente |
| --- | --- | --- |
| Janela e abas | Moldura de uma faixa, menu próprio, abas com ícones, modo privado, lista de abas, estante e restauração de abas fixadas | Múltiplas janelas, arrastar abas, grupos e restauração de abas comuns após reiniciar |
| Barra de endereço | Endereço, Google na pesquisa por padrão, escolha do buscador, Google na página inicial, sugestões de histórico e marcadores, voltar e avançar | Pesquisa por site e classificação avançada das sugestões |
| Marcadores | Barra e gerenciador com ícones, busca, edição, exclusão, menu do botão direito e arrastar para ordenar ou mover entre pastas | Importação, exportação e abertura de pastas em abas |
| Histórico | Página na aba, busca, data e hora, seleção por visita ou por dia e exclusão em lote | Filtros por data e site, histórico por grupo, painel lateral e retenção configurável |
| Preferências | Busca, modo claro e escuro, três cores, HTTPS, filtro, pop-ups, histórico, barra de marcadores, idioma dos sites e destino da tradução. As páginas internas compartilham uma aba | Inicialização, idioma da interface do Brisa, fontes, permissões e exceções persistentes por site |
| Ferramentas | DevTools do WebView2 encaixado ou em janela com o ícone do Brisa, zoom por Ctrl e roda com percentual na barra, tradução local na aba em seis idiomas | Busca na página, imprimir e salvar, detecção automática do idioma pelo conteúdo |
| Downloads | Confirmação e lista da sessão | Gerenciador persistente, destino configurável, progresso e retomada |
| Dados e conta | Perfil WebView2 local, modo privado, limpeza de dados | Controles de dados por site e período, perfis separados, importação, preenchimento e senhas, sincronização própria |
| Anúncios e rastreamento | EasyList e EasyPrivacy, regras de pop-ups publicitários, detalhes por destino no escudo e no contador clicável e pausa por origem | Cobertura de anúncios na própria página, atualização automática das listas, editor de regras e medição de falsos positivos |
| Segurança e compatibilidade | HTTPS por padrão, bloqueio de janelas automáticas, comunicação da aba de login com a de origem, validação de certificados do motor, páginas internas isoladas | Testar provedores externos, permissões por site, verificação de reputação e auditoria por cenário |
| Ecossistema | Renderização e DevTools do WebView2 | Verificar extensões compatíveis, aplicativos web, mídia, acessibilidade e integração como navegador padrão |
| Memória e disco | Dica de baixo consumo para abas de fundo, estante, filtro de pop-ups sob demanda, gravações agrupadas, limpeza isolada de cache | Medir sessões com várias abas, avaliar descarte de abas e políticas de armazenamento |

O Chrome exibiu visualizações do histórico por data e por grupo. Chrome e Brave exibiram busca em histórico, marcadores, downloads e configurações, além de importação de marcadores e configurações. As tarefas de interface usam o comportamento e os atalhos dos navegadores de referência. O ícone, a faixa de abas, as cores e as páginas internas seguem a identidade do Brisa.

A comparação da 0.6 registrou personalização de tema e barra de ferramentas e gerenciadores de extensões nos dois navegadores. O Brave também expôs opções de barra lateral e guias inativas. A captura em perfis temporários está em `evidence/reference-pages-v0.6.json`. A [revisão de segurança](SEGURANCA.md) documenta a dependência que impede a ativação de extensões no Brisa.

## Sessões e privacidade

O teste de integração confirmou uma sessão ativa em outra aba e após reiniciar o mesmo perfil. Sair invalidou o acesso. O recurso funcional de outra origem carregou e o servidor ficou sem receber as requisições do rastreador, do pop-up automático e do pop-up publicitário acionado por clique.

O provedor fictício manteve seu cookie entre duas aberturas, uma que abriu diretamente o endereço e outra que começou em branco. Ambas comunicaram o resultado à aba de origem e fecharam a própria aba. Os testes Rust cobriram transporte e escopo da exceção de pop-ups. Falta testar o isolamento dos cookies em abas privadas no fluxo completo e medir a compatibilidade com provedores de identidade reais.

## Retomada

- Executável atual em `target/release/brisa.exe`. O instalador é gerado em `target/distribution`. Console, Rede, encaixe e janela externa do DevTools foram verificados na 0.8 com um perfil descartável.
- A prova em `evidence/qa-v0.7.json` testa Google como buscador padrão, trocas entre quatro seções internas no mesmo documento, cookies, bloqueios, exclusão de histórico e persistência do tema.
- Na verificação visual, o contador abriu uma lista com o anúncio, o rastreador e o pop-up de um site local. A barra de endereços sugeriu um marcador local e abriu o endereço com as setas e Enter.
- O comparativo anterior em `evidence/benchmark-v0.6.json` mede Brisa 0.5 e 0.6, Chrome e Brave. A 0.6 ficou em 78,40 MiB de RAM privada e 5,32 MiB no perfil de disco. A 0.5 ficou em 76,28 MiB e 5,32 MiB.
- O RustSec verificou as dependências Rust do alvo Windows x64 com a base local `87025f6`. O relatório está em `evidence/rustsec-v0.6.json`.
- A importação e exportação interoperáveis de marcadores e a lista persistente de downloads são as próximas lacunas nos fluxos já comparados.

## Referências de comportamento

- [Atalhos do Chrome](https://support.google.com/chrome/answer/157179) · [Marcadores do Chrome](https://support.google.com/chrome/answer/188842) · [Desempenho do Chrome](https://support.google.com/chrome/answer/12929150)
- [Brave Shields](https://support.brave.app/hc/en-us/articles/360022806212-How-do-I-use-Shields-while-browsing) · [Proteção de cookies do Firefox](https://support.mozilla.org/en-US/kb/total-cookie-protection-and-website-breakage-faq)
- [Janelas abertas por páginas no WebView2](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2newwindowrequestedeventargs) · [Políticas de OAuth do Google](https://developers.google.com/identity/protocols/oauth2/policies)
