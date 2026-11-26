# Assinatura do instalador do Brisa com SignPath

O fluxo [Instalador Windows](../.github/workflows/windows-installer.yml) compila o Brisa em um executor Windows do GitHub Actions. Quando a opção **Solicitar assinatura do instalador pelo SignPath** é marcada, ele envia o instalador para aprovação e calcula o SHA-256 do arquivo assinado. [Integração do SignPath com GitHub Actions](https://docs.signpath.io/trusted-build-systems/github)

## Candidatura

- Publique o código-fonte com uma licença de código aberto aprovada pela OSI. No repositório do Brisa, o código, o manifesto Cargo e o fluxo do GitHub Actions ficam no mesmo nível dos arquivos do site. A compilação usa `src`, `assets`, `vendor`, `installer`, `docs`, `Cargo.toml`, `Cargo.lock`, `build.rs`, `build.ps1` e `package.ps1`.
- Use o resultado do fluxo **Instalador Windows** da branch principal para demonstrar a compilação na candidatura. O mesmo fluxo pode ser executado manualmente com a opção de assinatura desmarcada.
- Informe o repositório público e a página do Brisa no formulário. Descreva o projeto como um navegador para Windows, com interface em Rust e renderização pelo Microsoft Edge WebView2. Informe que o instalador inclui componentes oficiais da Microsoft e os componentes de terceiros descritos em [TERCEIROS.md](TERCEIROS.md).
- Após a aprovação, publique a política exigida pelo SignPath na página de download. Inclua a atribuição solicitada, os responsáveis por revisar alterações e aprovar assinaturas e a política de privacidade. As condições e a análise do projeto são da fundação. [Condições do programa](https://signpath.org/terms) · [Formulário de candidatura](https://signpath.org/apply)

## Configuração após a aprovação

No projeto aprovado do SignPath, configure a origem como o repositório do código do Brisa, use a política de assinatura de release da fundação e crie uma configuração de artefato com o slug `brisa-installer`. O fluxo envia um ZIP de artefato do GitHub que contém um instalador.

```xml
<artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
  <parameters>
    <parameter name="version" required="true" />
  </parameters>
  <zip-file>
    <pe-file path="Brisa-${version}-Windows-x64.exe" product-name="Brisa" product-version="${version}">
      <authenticode-sign />
    </pe-file>
  </zip-file>
</artifact-configuration>
```

No GitHub, configure `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG` e `SIGNPATH_SIGNING_POLICY_SLUG` como variáveis de Actions. Adicione o token de um usuário de CI com permissão de envio como segredo de Actions `SIGNPATH_API_TOKEN`. [Configuração de projetos e permissões](https://docs.signpath.io/projects)

## Release assinada

1. Crie a tag `vX.Y.Z` que corresponde à versão em `Cargo.toml`.
2. Execute **Instalador Windows** para essa tag e marque a opção de assinatura.
3. Aprove a solicitação no SignPath. O fluxo valida a assinatura Authenticode e o carimbo de tempo do instalador.
4. Baixe o artefato `brisa-windows-assinado`. Use o `.exe` e o `SHA256SUMS.txt` desse artefato ao publicar a release. Atualize a página do Brisa para indicar a assinatura da versão publicada.
