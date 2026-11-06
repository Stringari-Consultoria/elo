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
- **Som do computador acessado**: o que toca lá sai aqui (Opus, 96 kbps), com volume e botão de desligar.
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
- **Para empresas de suporte**: Elo Cliente (só recebe conexões, com aceite, e tem "Pedir ajuda"), contas de técnico, painel de clientes por empresa e convites.
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
| Elo Cliente (computadores atendidos por uma empresa) | `Elo-Cliente-…` / `elo-cliente…` | Veja [Empresas e Elo Cliente](#empresas-e-elo-cliente) |
| Só o servidor (máquina dedicada, VPS, Docker) | `elo-server…` | Veja [Servidor avulso](#3-servidor-avulso) |

Use um jeito de instalar só: se já instalou pelo `.tar.gz` (ou `make install`)
e passar para o `.deb`, rode antes `./instalar.sh --remover`, porque a cópia em
`~/.local` tem prioridade sobre a do sistema.

**No Linux** funciona no X11 e no **Wayland** (GNOME, KDE e outros com o
portal do sistema). No Wayland, na primeira vez que alguém acessa, o sistema
pergunta na tela se pode compartilhar: ligue **"Permitir interação remota"**
(sem isso dá para ver, mas não controlar) e deixe **"Lembrar desta seleção"**
marcado, para as próximas vezes não perguntarem de novo.

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

Para hospedar o servidor numa máquina dedicada (VPS, servidor da empresa,
Raspberry Pi), sem o app, use os pacotes `elo-server` da página de
[Releases](https://github.com/Stringari-Consultoria/elo/releases):

| Onde | Arquivo | Como instalar |
|---|---|---|
| Debian, Ubuntu (x86_64 ou ARM) | `elo-server_<versão>_amd64.deb` / `_arm64.deb` | `sudo apt install ./elo-server_<versão>_amd64.deb` |
| Outras distros Linux | `elo-server-<versão>-linux-x86_64.tar.gz` / `-aarch64` | Extraia e rode `sudo ./instalar.sh` |
| Docker (amd64, arm64) | `ghcr.io/stringari-consultoria/elo-server` | Veja abaixo |
| Windows / Windows Server | `elo-server-<versão>-windows-x64.zip` | Extraia; o `LEIA-ME.txt` mostra como iniciar com o Windows |
| macOS | `elo-server-<versão>-macos.tar.gz` | Extraia; o `LEIA-ME.txt` traz o serviço do `launchd` |

No Linux, o `.deb` e o `instalar.sh` deixam o servidor como **serviço do
systemd** (`elo-server`): sobe com a máquina, roda com um usuário próprio e
isolado e guarda os dados em `/var/lib/elo-server`. Ao terminar, mostram o
endereço e a **impressão digital** para colar nos apps (em **Configurações →
Servidor**, desligue "Rodar o servidor neste computador"). As opções ficam em
`/etc/default/elo-server`. Libere a porta `21116/TCP` no firewall.

```sh
sudo systemctl status elo-server       # situação
journalctl -u elo-server               # registros
sudo elo-server --impressao --data /var/lib/elo-server   # impressão digital de novo
```

**Docker:**

```sh
docker run -d --name elo-server --restart unless-stopped \
  -p 21116:21116 -v elo-dados:/dados ghcr.io/stringari-consultoria/elo-server
docker exec elo-server elo-server --impressao --data /dados
```

Com o Cloudflare Tunnel no mesmo `docker compose`, nem é preciso publicar a
porta: aponte o túnel para `https://elo-server:21116` (com **No TLS Verify**)
e use `wss://seu-endereço` nos apps.

```yaml
services:
  elo-server:
    image: ghcr.io/stringari-consultoria/elo-server
    restart: unless-stopped
    volumes: [elo-dados:/dados]
  cloudflared:
    image: cloudflare/cloudflared
    command: tunnel run
    environment: [TUNNEL_TOKEN=seu-token]
    restart: unless-stopped
volumes:
  elo-dados:
```

**À mão**, com qualquer um dos executáveis:

```sh
# certificado público (Let's Encrypt etc.)
elo-server --listen 0.0.0.0:21116 --data /var/lib/elo --cert fullchain.pem --key privkey.pem

# certificado autoassinado: cole nos apps a impressão digital que ele imprime
elo-server --listen 0.0.0.0:21116 --data /var/lib/elo --self-signed
```

O app de desktop já traz o servidor embutido (opção 1); o pacote do servidor
é só para quem quer uma máquina dedicada.

## Empresas e Elo Cliente

Para quem presta suporte a empresas: os computadores dos clientes ficam
organizados por empresa, os clientes pedem ajuda por um botão e só os seus
técnicos conseguem entrar, sempre com o aceite de quem está no computador.

**Técnicos.** No Elo, em **Configurações → Conta de técnico**, entre com
usuário e senha do servidor. Num servidor novo, a primeira conta vira
administrador com o **código de primeiro acesso**: ele aparece nos registros
do servidor (`journalctl -u elo-server`, `docker logs elo-server`) e no
arquivo `primeiro-acesso.txt` da pasta de dados; com o servidor embutido no
app, o próprio Elo já preenche. Administradores criam e removem técnicos na
aba **Clientes → Técnicos**. O servidor guarda só o hash (Argon2) das senhas,
e o app guarda só um token de entrada.

**Aba Clientes.** Pedidos de ajuda (com a mensagem do cliente e quem está
atendendo), empresas, computadores (online, visto pela última vez, nome e
observações) e o botão para conectar. Um pedido novo aparece na hora, com
aviso, mesmo com o Elo na bandeja.

**Convidar um computador.** Na empresa, **Convidar computador** gera um código
de convite (endereço do servidor, impressão digital e a chave da empresa) e
uma mensagem pronta com o link de download. O cliente instala o **Elo
Cliente**, cola o código e pronto. Para instalar em muitas máquinas, salve o
código em `convite.txt` em `C:\ProgramData\Elo\` (Windows), `/etc/elo/`
(Linux) ou `/Library/Application Support/Elo/` (macOS): o Elo Cliente lê
sozinho na primeira abertura. **Trocar o convite** invalida o código antigo
para instalações novas, sem derrubar os computadores já ligados.

**Elo Cliente.** É o mesmo programa, com outro nome e configuração própria (os
dois convivem na mesma máquina). Mostra o ID e o botão **Pedir ajuda**; não
conecta em outros computadores e não aceita senhas: todo acesso é aceito na
tela, e quem aceita vê o nome do técnico. O servidor também garante isso:
computadores de empresa só recebem conexões, e só de técnicos logados. O
histórico de sessões registra qual técnico fez cada acesso.

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
- **Som:** o anfitrião captura o que está saindo nas caixas (PipeWire ou
  PulseAudio no Linux, loopback do WASAPI no Windows, process tap do Core
  Audio no macOS 14.2+), comprime em Opus (48 kHz, estéreo, 20 ms, codec em
  Rust puro) e o visualizador toca com um pequeno buffer contra a variação
  da rede. Versões novas anunciam o que sabem fazer logo depois da mensagem
  de boas-vindas, e versões antigas simplesmente ignoram: o som só é pedido
  a quem anunciou.

## Desenvolvimento

Requisitos: **Rust 1.88+** ([rustup](https://rustup.rs)). No Linux (Debian/Ubuntu):

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgtk-3-dev libxcb1-dev libxdo-dev libasound2-dev libpipewire-0.3-dev libclang-dev
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
- **Wayland:** testado no GNOME 46 (numa sessão aninhada); KDE e outros
  ambientes com o portal devem funcionar, mas ainda não foram testados. Se a
  permissão guardada deixar de valer (por exemplo, depois de revogada nos
  Ajustes), o sistema pergunta de novo na tela do computador acessado: para
  acesso sem ninguém lá, prefira uma sessão X11 nesse computador.
- **Sem tela de login.** Não há acesso antes do login, e o Ctrl+Alt+Del do
  Windows exige um serviço do sistema que o Elo ainda não tem.
- **Som no Mac acessado** exige o macOS 14.2 ou mais novo e a permissão
  "Gravação de áudio do sistema". No Linux, precisa do `pw-record` (PipeWire)
  ou do `parec` (PulseAudio), que já vêm nas distribuições comuns.
- **Ainda não há:** gravação de sessão, impressão remota, nem
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
