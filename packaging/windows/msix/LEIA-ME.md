# Elo na Microsoft Store

Pacote MSIX, sem assinatura: a loja assina ao publicar, então não precisa de
certificado. O pacote é gerado pelo [`scripts/pacote-msix.ps1`](../../../scripts/pacote-msix.ps1)
e pelo CI (job `windows` do Release, artefato `loja-windows`).

Dentro do pacote, o Elo:

- inicia com o computador pela tarefa de inicialização do Windows
  (`EloInicio`), e não pelo `Run` do registro;
- não se atualiza sozinho (a loja atualiza, como pede a política dela);
- guarda as configurações na pasta do pacote, que o Windows apaga ao
  desinstalar.

## Uma vez só

1. **Partner Center → Aplicativos e jogos → Novo produto → Aplicativo MSIX ou
   PWA.** Reserve o nome. Se "Elo" estiver em uso, use outro, como
   "Elo Acesso Remoto".
2. **Produto → Gerenciamento do produto → Identidade do produto.** Copie os
   valores para as variáveis do repositório (GitHub → Settings → Secrets and
   variables → Actions → Variables):

   | Variável            | Campo no Partner Center              |
   |---------------------|--------------------------------------|
   | `MSIX_NOME`         | Package/Identity/Name                |
   | `MSIX_EDITOR`       | Package/Identity/Publisher (`CN=…`)  |
   | `MSIX_EDITOR_NOME`  | Package/Properties/PublisherDisplayName |
   | `MSIX_NOME_EXIBIDO` | o nome reservado, exatamente igual   |

   Sem essas variáveis, o CI gera um pacote de teste, que a loja recusa.

## A cada versão

1. Publique a versão normalmente (tag `vX.Y.Z`).
2. No Release dessa tag, baixe o artefato **loja-windows**
   (`Elo-X.Y.Z-windows-x64.msix`).
3. No Partner Center, abra um **novo envio**, troque o pacote em
   **Pacotes** e envie.

## Primeiro envio: o que preencher

**Preços e disponibilidade:** gratuito, todos os mercados.

**Propriedades:** categoria *Utilitários e ferramentas*. O app tem política de
privacidade, então informe a URL
`https://github.com/Stringari-Consultoria/elo/blob/main/PRIVACIDADE.md`. Em
requisitos do sistema, deixe teclado e mouse como recomendados.

**Classificação etária:** responda o questionário IARC. O Elo tem chat entre
usuários e compartilhamento de tela, mas não tem conteúdo próprio.

**Opções de envio → Recursos restritos (`runFullTrust`):**

> Elo is a remote desktop app (open source, AGPL-3.0). It is a Win32 desktop
> application packaged as MSIX, so it needs runFullTrust to capture the screen
> (DXGI), inject mouse and keyboard input (SendInput) for the remote
> technician, and run its built-in connection server. Remote control only
> happens during a session the user accepted on screen or allowed with their
> own unattended-access password. Source code:
> https://github.com/Stringari-Consultoria/elo

**Notas para certificação:**

> No account or login is needed. On launch, Elo shows this computer's ID and a
> temporary password, using the connection server built into the app.
> To test a remote session, install Elo on two Windows computers on the same
> network.
> 1. On computer A, open Settings (gear icon). Under "On the other computers,
>    turn off 'Run the server' and use:" you'll see the local network address
>    (for example 192.168.0.10:21116) and the certificate fingerprint.
> 2. On computer B, in the wizard's "Server" step, choose "Connect to an
>    existing server" and enter that address and fingerprint.
> 3. On B, type A's ID and temporary password (both shown on A's main screen)
>    and click Connect. With no password, A shows an access request to accept.
> The Windows Firewall prompt on first launch is for the built-in server
> (incoming connections on the local network).

**Listagem da loja (pt-BR):**

- *Descrição:*

  > Acesse e controle outro computador com cifragem ponta a ponta. O Elo mostra
  > a tela, o som, o mouse e o teclado do outro computador, com chat, área de
  > transferência e envio de arquivos.
  >
  > • Sem conta e sem servidor de terceiros: o servidor de conexão vem embutido
  > no app, ou use o servidor da sua empresa.
  > • Cifragem ponta a ponta (SPAKE2 + ChaCha20-Poly1305): o servidor só repassa
  > dados que não consegue ler.
  > • Acesso não assistido com senha fixa, e pedido de acesso com aceite na tela
  > e código de verificação.
  > • Para empresas de suporte: painel de clientes, contas de técnicos e o Elo
  > Cliente.
  > • Código aberto (AGPL-3.0), também para Linux e macOS.

- *Recursos do produto:* Cifragem ponta a ponta; Servidor de conexão embutido;
  Acesso não assistido; Chat, área de transferência e arquivos; Várias telas;
  Código aberto.
- *Palavras-chave:* acesso remoto, área de trabalho remota, suporte remoto,
  controle remoto, código aberto.
- *Capturas de tela:* no mínimo 1366×768. Para gerar a partir de
  `docs/prints` (requer Pillow):
  `python3 scripts/loja-capturas.py docs/prints <pasta> 01-inicio.png 08-tecnico-sessao.png 07-tecnico-painel-de-clientes.png 10-cliente-pedir-ajuda.png 03-configuracoes.png`

Os ícones em `Assets/` saem de `python3 scripts/msix-imagens.py packaging/icons/elo-1024.png packaging/windows/msix/Assets`.

**Listagem (en-US):** a mesma, traduzida. O app já vem em inglês quando o
Windows não está em português.

## Testar o pacote fora da loja

Pelo modo de desenvolvedor, sem assinatura: extraia o `.msix` (é um zip),
apague `AppxBlockMap.xml` e `[Content_Types].xml`, e registre:

```powershell
Add-AppxPackage -Register .\AppxManifest.xml
```
