# Elo by Stringari

Suporte e acesso remoto: um computador vê e controla a tela de outro, com chat,
área de transferência e envio de arquivos. É a mesma proposta do TeamViewer e do
AnyDesk, escrita em Rust (egui) com o design system Stringari.

## Como funciona

1. Quem precisa de ajuda abre o Elo e passa o **ID** (9 dígitos) e a **senha temporária**.
2. Quem ajuda digita o ID (e a senha, se tiver) e clica em **Conectar**.
3. Sem senha, aparece um **pedido de acesso** na tela do outro lado, com um código
   de verificação e as permissões (controle, área de transferência, arquivos).
4. Durante a sessão, uma faixa amarela avisa **"Este computador está sendo acessado
   por …"** e tem um botão **Encerrar**. As permissões podem ser retiradas no meio da sessão.

**Acesso não assistido:** defina uma senha fixa (8+ caracteres) nas configurações
e ligue a chave na tela inicial. Com ela, a conexão entra sem ninguém do outro lado.

## Segurança

- **Ponta a ponta.** As duas pontas fazem um acordo de chave SPAKE2 (Ed25519) a
  partir da senha e trocam tudo cifrado com ChaCha20-Poly1305. O servidor só
  repassa bytes: não tem a chave, não lê a sessão e não consegue se passar por
  nenhum dos lados sem saber a senha.
- **Código de verificação** de 6 dígitos nas duas telas. Se for igual, ninguém
  está no meio. Vale principalmente nas conexões por pedido (sem senha).
- **Senha errada:** após 5 tentativas em 5 minutos, o computador recusa novas
  conexões por um tempo. O servidor também limita tentativas por IP.
- **TLS** entre os apps e o servidor (certificado público ou autoassinado com
  impressão digital fixada nas configurações).
- O arquivo de configuração (que guarda o segredo do dispositivo e a senha fixa)
  é gravado com permissão 0600 no Linux e no macOS.

### LGPD: o que fica no servidor

O servidor registra **apenas metadados**: ID, nome e sistema de cada computador,
IP de origem, início, fim e volume de dados de cada sessão (`sessoes.jsonl`).
Telas, teclas, arquivos, chat e área de transferência **não passam em claro**
pelo servidor. Esses metadados são dados pessoais: defina prazo de retenção
e informe na política de privacidade do serviço.

## Estrutura

| Pasta | O que é |
|---|---|
| `apps/elo` | App desktop (egui/wgpu) |
| `apps/elo-server` | Servidor de rendezvous e retransmissão (tokio): biblioteca embutida no app e binário avulso |
| `crates/proto` | Protocolo: mensagens, handshake SPAKE2, cifragem, codec de vídeo |
| `crates/core` | Captura de tela, injeção de entrada, cliente e sessões |
| `crates/ui-kit` | Design system Stringari (o mesmo do Traço) |

**Vídeo:** a tela é dividida em blocos de 64 px; só os blocos que mudaram vão em
JPEG. Quando a tela para, os blocos são refinados em qualidade alta. Há controle
de fluxo (até 2 quadros sem confirmação), então a sessão se adapta a links lentos.

## Rodando

```sh
cargo run -p elo --release
```

**O servidor já vem junto.** Ao abrir, o Elo sobe o servidor dentro do próprio
app (porta `21116`, TLS com certificado autoassinado guardado na pasta de
configuração, em `servidor/`) e conecta nele. Não precisa de terminal. Ao
fechar o app, o servidor para.

Para os outros computadores usarem esse servidor: em **Configurações → Servidor**
deste computador aparecem o endereço (`IP:21116`) e a impressão digital do
certificado. Nos outros, desligue **"Rodar o servidor neste computador"** e
preencha endereço, TLS e impressão digital. A porta precisa estar liberada no
firewall (no Windows, aceite o aviso do firewall na primeira abertura).

Desligando a opção, o app conecta em qualquer outro servidor (o padrão é
`elo.stringari.com.br:21116`, com TLS e certificado público).

### Pela internet com Cloudflare Tunnel

O servidor também aceita **WebSocket** na mesma porta, então funciona atrás do
Cloudflare Tunnel sem instalar nada nos outros computadores.

1. No túnel, crie um *Public Hostname* (ex.: `elo.seudominio.com`), tipo
   **HTTP**, URL `localhost:21116`. Em YAML:

   ```yaml
   ingress:
     - hostname: elo.seudominio.com
       service: http://localhost:21116
     - service: http_status:404
   ```

   Sem TLS, o servidor só aceita conexões vindas do próprio computador (o
   `cloudflared`); o TLS fica com o Cloudflare. Se preferir `https://localhost:21116`,
   ligue `noTLSVerify` (certificado autoassinado).
2. No Elo deste computador, em **Configurações → Servidor**, preencha
   **Endereço público** com `wss://elo.seudominio.com`. Para não deixar a porta
   aberta na rede local, desligue **"Aceitar conexões diretas da rede"**.
3. Nos outros computadores: desligue **"Rodar o servidor neste computador"** e
   use o endereço `wss://elo.seudominio.com`, com a impressão digital vazia.

O servidor usa o IP real do cliente (`CF-Connecting-IP`) nos limites de
tentativas e nos registros, e manda ping a cada 30 s para o Cloudflare não
fechar conexões paradas. Atenção: toda a sessão (tela inclusive) passa pelo
Cloudflare; uso pesado de banda pode esbarrar nos termos do plano gratuito.

### Servidor avulso (opcional)

O mesmo servidor também roda sozinho, para hospedar em uma máquina dedicada:

```sh
# testes locais
cargo run -p elo-server -- --listen 127.0.0.1:21116 --data ./dados --no-tls

# produção: certificado público
elo-server --listen 0.0.0.0:21116 --data /var/lib/elo --cert fullchain.pem --key privkey.pem
```

Com `--self-signed` (padrão sem `--cert`), cole nos apps a impressão digital que
o servidor imprime ao iniciar.

`ELO_CONFIG_DIR` muda a pasta de configuração (útil para rodar duas instâncias
na mesma máquina: na segunda, desligue o servidor local e aponte para
`127.0.0.1:21116` com a impressão digital da primeira).

## Limitações (leia antes de vender)

- **Só retransmissão.** Todo o tráfego passa pelo servidor; não há conexão
  direta P2P (hole punching). Funciona atrás de qualquer NAT, mas o custo de
  banda do servidor cresce com o uso, e a latência depende da distância até ele.
- **Windows e macOS não foram compilados nem testados aqui.** O código usa xcap
  e enigo nessas plataformas; o CI compila nos três sistemas, mas o
  comportamento real (permissões de Gravação de Tela e Acessibilidade no macOS,
  telas UAC no Windows) precisa ser testado em máquinas reais.
- **Linux: só X11.** Wayland bloqueia captura e injeção de entrada sem portal
  (xdg-desktop-portal/PipeWire), que ainda não foi implementado.
- **Ctrl+Alt+Del:** no Linux é enviado como atalho comum. No Windows a tela
  segura (SAS) só pode ser chamada por um serviço do sistema; o Elo ainda não
  tem esse serviço. Também não há acesso à tela de login nem antes do login.
- **Não há:** áudio remoto, gravação de sessão, impressão remota, reinício com
  reconexão automática, lista de contatos/conta na nuvem, nem instalador ou
  assinatura de código.
- Teclados com layout diferente nos dois lados: o texto é enviado como texto
  (funciona), mas atalhos com teclas fora do ASCII podem não chegar.
- O codec é JPEG por blocos (simples e robusto). Em telas com vídeo em
  movimento usa mais banda que H.264/VP9.
