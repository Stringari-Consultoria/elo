<p align="center">
  <img src="packaging/icons/elo-256.png" width="96" height="96" alt="Ícone do Elo">
</p>

<h1 align="center">Elo</h1>

<p align="center">
  Acesso remoto e suporte técnico, com cifragem ponta a ponta e servidor próprio.<br>
  Feito em Rust. Sem conta, sem nuvem obrigatória.
</p>

<p align="center">
  <a href="https://github.com/Stringari-Consultoria/elo/actions/workflows/ci.yml"><img src="https://github.com/Stringari-Consultoria/elo/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="#licença"><img src="https://img.shields.io/badge/licen%C3%A7a-MIT%20ou%20Apache--2.0-blue" alt="Licença: MIT ou Apache-2.0"></a>
  <img src="https://img.shields.io/badge/rust-1.88%2B-orange" alt="Rust 1.88+">
</p>

---

O Elo deixa um computador ver e controlar a tela de outro, com chat, área de
transferência e envio de arquivos. A proposta é a mesma de ferramentas como
TeamViewer e AnyDesk, com duas diferenças. **O servidor é seu**: ele vem
embutido no app, ou você hospeda o seu. E **ninguém no meio consegue ler a
sessão**, nem o próprio servidor.

> **Estado do projeto: alfa.** Testado em máquinas reais no Linux (X11) e no
> macOS, com testes de ponta a ponta no CI. O Windows compila e passa nos testes
> do CI, mas ainda não foi testado em máquina real. Leia as
> [limitações](#limitações-conhecidas) antes de usar em produção.

## Recursos

- **Controle remoto** de mouse e teclado, com vários monitores.
- **Chat e área de transferência** nos dois sentidos.
- **Gerenciador de arquivos**: navegue nas pastas do outro computador, baixe e
  envie arquivos (ou arraste para a janela da sessão).
- **Cifragem ponta a ponta**: SPAKE2 a partir da senha, depois ChaCha20-Poly1305.
- **Código de verificação** de 6 dígitos nas duas telas, contra ataques no meio.
- **Pedido de acesso**: sem senha, o outro lado aceita na tela e escolhe as permissões.
- **Acesso não assistido** com senha fixa.
- **Permissões revogáveis** durante a sessão (controle, área de transferência, arquivos).
- **Servidor embutido**: o app já sobe o servidor ao abrir, sem configurar nada.
- **Assistente na primeira abertura**: escolha entre ser o servidor ou conectar a um, com teste.
- **Favoritos** com nome personalizado, **tela cheia** e **iniciar com o computador**.
- **Atualização automática**, com pacotes assinados e conferidos antes de instalar.
- **Wake-on-LAN**: liga um computador desligado da lista de favoritos ou recentes.
- **Funciona atrás do Cloudflare Tunnel** (WebSocket), sem abrir portas no roteador.
- **Leve com links lentos**: só as partes da tela que mudaram são enviadas, com controle de fluxo.

## Instalação

Baixe o instalador na página de [**Releases**](https://github.com/Stringari-Consultoria/elo/releases).

| Sistema | Arquivo | Como instalar |
|---|---|---|
| Ubuntu, Debian e derivados | `elo_<versão>_amd64.deb` | `sudo apt install ./elo_<versão>_amd64.deb` |
| Outras distros Linux | `elo-<versão>-linux-x86_64.tar.gz` | Extraia e rode `./instalar.sh` (instala em `~/.local`, sem root) |
| macOS 11+ (Intel e Apple Silicon) | `Elo-<versão>-macos.dmg` | Abra e arraste o **Elo** para **Aplicativos** |
| Windows 10 e 11 (64 bits) | `Elo-<versão>-windows-x64-setup.exe` | Rode o instalador (não precisa de administrador) |
| Windows, sem instalar | `Elo-<versão>-windows-x64.zip` | Extraia e abra o `elo.exe` |

**No Linux** a sessão precisa ser **X11**. No Ubuntu, escolha "Ubuntu no Xorg"
na tela de login. Wayland ainda não é suportado.

**No macOS**, os builds ainda não são assinados nem notarizados pela Apple, e
na primeira abertura aparece "a Apple não pôde verificar". Para liberar: tente
abrir o Elo uma vez, vá em **Ajustes do Sistema → Privacidade e Segurança**,
role até o aviso sobre o Elo e clique em **Abrir Mesmo Assim**. Para ser
acessado, o Elo também precisa das permissões de **Gravação de Tela** e
**Acessibilidade**, na mesma tela de ajustes.

**No Windows**, os builds ainda não são assinados. Se o SmartScreen avisar,
clique em **Mais informações → Executar assim mesmo**. Na primeira abertura, o
Windows pergunta se o Elo pode receber conexões da rede: permita, para que o
servidor embutido funcione.

Para compilar a partir do código, veja [Desenvolvimento](#desenvolvimento).

## Como usar

1. Quem precisa de ajuda abre o Elo e passa o **ID** (9 dígitos) e a **senha temporária**.
2. Quem ajuda digita o ID e a senha e clica em **Conectar**.
3. Sem a senha, aparece um **pedido de acesso** do outro lado, com o código de
   verificação e as permissões. Confira se o código é igual nas duas telas.
4. Durante a sessão, uma faixa avisa **"Este computador está sendo acessado
   por …"**, com o botão **Encerrar**.

**Acesso não assistido:** defina uma senha fixa (8 ou mais caracteres) nas
configurações e ligue a opção na tela inicial. A conexão então entra sem
ninguém do outro lado.

**Atualizações:** o Elo procura versão nova ao abrir e a cada 6 horas. Com
"Atualizar automaticamente" ligado (padrão), baixa sozinho e instala quando não
há sessão em andamento, reabrindo em seguida. No Linux instalado pelo `.deb`,
pede a senha de administrador; o `.zip` portátil do Windows só avisa.

**Arquivos:** na sessão, clique em **Arquivos** para ver as pastas do outro
computador. Clique duplo abre a pasta ou baixa o arquivo; **Enviar para cá**
manda arquivos deste computador para a pasta aberta. Também dá para arrastar
arquivos para a janela. Os baixados vão para a pasta de recebidos
(Configurações). Só funciona com a permissão "Transferir arquivos" ligada do
outro lado.

**Favoritos:** na sessão, clique na estrela. Na tela inicial, os favoritos
podem ser renomeados; um clique preenche o ID, e o clique duplo já conecta.

**Iniciar com o computador:** ligue em Configurações (ou no assistente). O Elo
abre na bandeja ao entrar no sistema, e o computador continua acessível depois
de reiniciar.

**Ligar um computador (Wake-on-LAN):** passe o mouse num cartão de favorito ou
recente e clique no botão de ligar. O servidor manda o "pacote mágico" na rede
dele e pede o mesmo aos Elos online na rede do computador desligado (mesmo IP
público). Requisitos: o computador precisa ter aberto a versão nova do Elo ao
menos uma vez (para o servidor conhecer o endereço da placa de rede), o
Wake-on-LAN ligado na BIOS/placa e, em geral, cabo de rede; e alguém (o
servidor ou outro Elo) precisa estar ligado na mesma rede.

**Reiniciar e reconectar:** na sessão, "Reiniciar" reinicia o outro
computador (com a permissão de controle). O Elo tenta reconectar sozinho por
até 6 minutos. Para voltar sem ninguém lá, o outro computador precisa iniciar o
Elo com o computador e ter o acesso não assistido ligado (a senha temporária
muda ao reiniciar).

**Bandeja:** fechar a janela deixa o Elo rodando perto do relógio, aceitando
conexões. O menu do ícone mostra o seu ID e tem "Abrir", "Copiar ID" e "Sair do
Elo". Quando chega um pedido de acesso ou começa uma sessão, a janela aparece
sozinha. No Linux, usa StatusNotifierItem (KDE, e GNOME com a extensão
AppIndicator, padrão no Ubuntu).

## Servidor

O servidor guarda os IDs, entrega os pedidos de conexão e repassa os bytes das
sessões. Ele nunca tem a chave da sessão. Existem três jeitos de usar:

### 1. Embutido no app (padrão)

Ao abrir, o Elo sobe o servidor na porta `21116`, com TLS e certificado
autoassinado, e conecta nele. Ao fechar, o servidor para.

Em **Configurações → Servidor** aparecem o endereço deste computador na rede e
a impressão digital do certificado, com botões para copiar. Nos outros
computadores, desligue **"Rodar o servidor neste computador"** e preencha esses
dados. A porta `21116` precisa estar liberada no firewall.

### 2. Pela internet com Cloudflare Tunnel

O servidor aceita **WebSocket** na mesma porta. Por isso funciona atrás do
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
sem abrir portas e sem instalar nada nos outros computadores.

1. No túnel, crie um *Public Hostname*, por exemplo `elo.seudominio.com`, do
   tipo **HTTP**, apontando para `localhost:21116`:

   ```yaml
   ingress:
     - hostname: elo.seudominio.com
       service: http://localhost:21116
     - service: http_status:404
   ```

2. No Elo que roda o servidor, em **Configurações → Servidor**, preencha
   **Endereço público** com `wss://elo.seudominio.com`. Se não precisar de
   acesso direto pela rede local, desligue **"Aceitar conexões diretas da rede"**.
3. Nos outros computadores, desligue o servidor local e use
   `wss://elo.seudominio.com` como endereço, com a impressão digital vazia.

Conexões sem TLS só são aceitas quando vêm do próprio computador, que é o caso
do `cloudflared`. O servidor usa o IP real do cliente (`CF-Connecting-IP`) nos
limites de tentativas. Atenção: a sessão inteira, tela incluída, passa pelo
Cloudflare. Uso pesado de banda pode esbarrar nos termos do plano gratuito.

### Painel do servidor

Com o servidor rodando neste computador, **Configurações → Servidor → Painel do
servidor** mostra quem está online (nome, sistema, IP), as sessões em andamento
e o histórico (quem acessou quem, quando, duração e volume). O histórico é
apagado depois do prazo configurado (90 dias por padrão; no servidor avulso,
`--retencao-dias`).

### 3. Servidor avulso

O mesmo servidor também roda sozinho, para hospedar numa máquina dedicada:

```sh
# certificado público (Let's Encrypt etc.)
elo-server --listen 0.0.0.0:21116 --data /var/lib/elo --cert fullchain.pem --key privkey.pem

# certificado autoassinado: cole nos apps a impressão digital que ele imprime
elo-server --listen 0.0.0.0:21116 --data /var/lib/elo --self-signed
```

O `elo-server` vem junto nos instaladores.

## Segurança

- **Ponta a ponta.** As duas pontas fazem um acordo de chave SPAKE2 (Ed25519) a
  partir da senha e trocam tudo cifrado com ChaCha20-Poly1305. O servidor só
  repassa bytes: não tem a chave, não lê a sessão e não consegue se passar por
  nenhum dos lados sem saber a senha.
- **Código de verificação** de 6 dígitos nas duas telas. Se for igual, não há
  ninguém no meio. É especialmente importante nas conexões por pedido (sem senha).
- **Senha errada:** depois de 5 tentativas em 5 minutos, o computador recusa
  novas conexões por um tempo. O servidor também limita tentativas por IP.
- **TLS** entre o app e o servidor, com certificado público ou com impressão
  digital fixada.
- O arquivo de configuração, que guarda o segredo do dispositivo e a senha
  fixa, é gravado com permissão `0600` no Linux e no macOS.

Encontrou uma vulnerabilidade? **Não abra uma issue pública.** Veja
[SECURITY.md](SECURITY.md).

### Privacidade (LGPD)

O servidor registra **apenas metadados**: ID, nome e sistema de cada
computador, IP de origem, e início, fim e volume de cada sessão (em
`sessoes.jsonl`). Telas, teclas, arquivos, chat e área de transferência nunca
passam em claro pelo servidor. Se você oferece o Elo como serviço, esses
metadados são dados pessoais: defina um prazo de retenção e informe na sua
política de privacidade.

## Como funciona por dentro

| Pasta | O que é |
|---|---|
| [`apps/elo`](apps/elo) | App desktop (egui + wgpu) |
| [`apps/elo-server`](apps/elo-server) | Servidor de IDs e retransmissão (tokio). É biblioteca, embutida no app, e também binário avulso |
| [`crates/proto`](crates/proto) | Protocolo: mensagens, handshake SPAKE2, cifragem, codec de vídeo |
| [`crates/core`](crates/core) | Captura de tela, injeção de entrada, cliente e sessões |
| [`crates/ui-kit`](crates/ui-kit) | Design system Stringari |
| [`packaging`](packaging) e [`scripts`](scripts) | Ícones, `Info.plist`, script do Inno Setup e scripts dos instaladores |

- **Conexão:** cada app mantém uma conexão de controle com o servidor (TCP +
  TLS, ou WebSocket). Para abrir uma sessão, o servidor gera um convite e as
  duas pontas entram no relay. A partir daí, o servidor só copia bytes.
- **Vídeo:** a tela é dividida em blocos de 64 px, e só os blocos que mudaram
  vão em JPEG. Quando a tela para, os blocos são refinados em qualidade alta.
  Com até 2 quadros sem confirmação, a sessão se adapta a links lentos.

## Desenvolvimento

Requisitos: **Rust 1.88+** ([rustup](https://rustup.rs)). No Linux (Debian/Ubuntu):

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgtk-3-dev libxcb1-dev libxdo-dev
```

Os atalhos ficam no `Makefile` (`make` lista todos):

| Comando | O que faz |
|---|---|
| `make run` | Abre o app (release) |
| `make run-dev` | Abre em debug, com configuração separada em `.dev-config/` |
| `make server` | Servidor avulso local, sem TLS, para testes |
| `make ci` | Clippy e testes, igual ao CI |
| `make build` | Executáveis em `dist/` |
| `make install` | Instala em `~/.local/bin`, com atalho no menu (Linux) |
| `make pacote-linux` | Gera `.deb` e `.tar.gz` em `dist/` |
| `make pacote-macos` | Gera o `.dmg` em `dist/` (só no macOS) |
| `make pacote-windows` | Gera o instalador `.exe` e o `.zip` em `dist/` (só no Windows, com [Inno Setup 6](https://jrsoftware.org/isinfo.php)) |

Os testes de ponta a ponta sobem um servidor de verdade e fazem sessões
completas pelo relay, também via WebSocket. As partes que capturam a tela usam
o X11 de `DISPLAY` e são puladas sem ele.

`ELO_CONFIG_DIR` muda a pasta de configuração. Serve para rodar duas instâncias
na mesma máquina: na segunda, desligue o servidor local e aponte para
`127.0.0.1:21116` com a impressão digital da primeira.

**Assinatura no macOS:** o workflow de release assina com Developer ID e
notariza quando estes *secrets* estão configurados no repositório (exige o
[Apple Developer Program](https://developer.apple.com/programs/)):

| Secret | Conteúdo |
|---|---|
| `APPLE_CERT_P12` | Certificado *Developer ID Application* exportado em `.p12`, em base64 |
| `APPLE_CERT_PASSWORD` | Senha do `.p12` |
| `APPLE_SIGN_IDENTITY` | Nome do certificado, ex.: `Developer ID Application: Nome (TEAMID)` |
| `APPLE_ID`, `APPLE_TEAM_ID` | Conta Apple e Team ID, para a notarização |
| `APPLE_APP_PASSWORD` | Senha de app gerada em [account.apple.com](https://account.apple.com) |

Sem eles, o app sai com assinatura ad hoc (e o aviso acima).

**Atualização automática:** cada release publica `latest.json` (versão e
SHA-256 de cada pacote) e `latest.json.sig`, a assinatura Ed25519 do manifesto
feita no CI com o secret `ELO_UPDATE_KEY`. O app só instala se a assinatura
conferir com a chave pública embutida (`apps/elo/src/update.rs`) e se o pacote
baixado bater com o SHA-256. Guarde um backup da chave privada: sem ela, as
versões instaladas não aceitam novas atualizações. A tag precisa ser igual à
versão do `Cargo.toml` (o CI confere).

**Releases:** ao enviar uma tag `v*`, o workflow
[`release.yml`](.github/workflows/release.yml) gera o `.deb` e o `.tar.gz`
(no Ubuntu 22.04, para funcionar em distros mais antigas), o `.dmg` universal e
o instalador e o `.zip` do Windows, e publica a release.

## Limitações conhecidas

- **Só retransmissão.** Todo o tráfego passa pelo servidor; não há conexão
  direta P2P (hole punching). Funciona atrás de qualquer NAT, mas a banda do
  servidor cresce com o uso e a latência depende da distância até ele.
- **Windows ainda não foi testado em máquina real.** O CI compila e roda os
  testes nos três sistemas; o Linux e o macOS já foram testados em máquinas
  reais. Telas UAC do Windows precisam de teste.
- **Linux: só X11.** No Wayland, captura e controle exigem o portal
  (xdg-desktop-portal/PipeWire), que ainda não foi implementado.
- **Sem tela de login.** Não há acesso antes do login, e o Ctrl+Alt+Del do
  Windows exige um serviço do sistema que o Elo ainda não tem.
- **Ainda não há:** áudio remoto, gravação de sessão, impressão remota, nem
  builds assinados pela Apple ou pela Microsoft.
- Com layouts de teclado diferentes nos dois lados, o texto chega certo, mas
  atalhos com teclas fora do ASCII podem não chegar.
- O codec é JPEG por blocos (simples e robusto). Com vídeo em movimento na
  tela, gasta mais banda que H.264 ou VP9.

Quer ajudar com algum desses itens? Veja abaixo.

## Contribuindo

Contribuições são bem-vindas: correções, testes no Windows, suporte a
Wayland, melhorias no codec. Leia o [CONTRIBUTING.md](CONTRIBUTING.md) antes de
abrir um pull request.

## Licença

O código é licenciado, à sua escolha, sob a [MIT](LICENSE-MIT) ou a
[Apache 2.0](LICENSE-APACHE). Salvo declaração em contrário, toda contribuição
enviada para inclusão neste projeto é licenciada da mesma forma, sem termos
adicionais.

As fontes em `assets/fonts` (Geist e JetBrains Mono) são distribuídas sob a SIL
Open Font License 1.1. Veja [NOTICE](NOTICE).

TeamViewer e AnyDesk são marcas de seus respectivos donos. O Elo não é afiliado
a eles nem endossado por eles.

---

<p align="center">Feito por <a href="https://github.com/Stringari-Consultoria">Stringari Consultoria</a>.</p>
