# Política de privacidade do Elo

*Última atualização: 17 de novembro de 2026* · [English](PRIVACY.md)

O Elo é um app de acesso remoto de código aberto, publicado pela Stringari
Consultoria. Esta política vale para o app em todas as lojas e downloads.

## O que a Stringari Consultoria coleta

**Nada.** O Elo não tem conta na Stringari, não manda telemetria, estatísticas
de uso nem relatórios de erro, e não passa por nenhum servidor nosso. Não
vendemos nem compartilhamos dados, porque não os recebemos.

## Por onde os dados passam

Para conectar dois computadores, o Elo usa um **servidor de conexão escolhido
por quem usa o app**: o servidor embutido no próprio Elo, ou um servidor da
sua empresa (`elo-server`). Esse servidor é de quem o instala, não da
Stringari.

- **Tela, áudio, teclado, mouse, área de transferência, chat e arquivos** da
  sessão são cifrados ponta a ponta (SPAKE2 + ChaCha20-Poly1305) entre os dois
  computadores. O servidor de conexão só repassa os dados cifrados e não
  consegue lê-los.
- **O servidor de conexão guarda**, na máquina dele:
  - o ID, o nome, o sistema, o endereço MAC e o endereço IP de cada
    computador que se conecta a ele, e quando foi visto pela última vez;
  - o histórico de sessões: quem acessou quem, o início, o fim e o volume de
    dados (por 90 dias, por padrão, e quem administra o servidor pode mudar o prazo);
  - as contas de técnicos e as empresas cadastradas, se o recurso for usado.

Quem administra o servidor de conexão (você ou a sua empresa) é responsável
por esses dados.

## O que fica no seu computador

As configurações do Elo (nome do computador, servidor, preferências) ficam
numa pasta do seu usuário e não saem dela. A senha de acesso não assistido
não fica guardada: o Elo guarda só um hash dela (Argon2id, com sal), e nem a
senha nem o hash são enviados a ninguém, nem ao servidor de conexão. Eles só
servem para provar a identidade dentro da cifragem (SPAKE2). Apagar essa pasta remove tudo (na
versão da Microsoft Store, desinstalar já apaga).

## Outras conexões

- **Atualizações:** as versões baixadas do GitHub consultam
  `github.com/Stringari-Consultoria/elo` para saber se há versão nova. A
  versão da Microsoft Store não faz isso, porque quem atualiza é a loja.
- **Teste do endereço público:** quando você pede, o Elo tenta se conectar ao
  endereço público que você mesmo configurou.

## Permissões

O Elo captura a tela, o áudio e controla o mouse e o teclado **só durante uma
sessão** que você aceitou na tela ou liberou com a senha de acesso não
assistido. Durante a sessão, o Elo mostra quem está acessando, e você pode
encerrar a sessão a qualquer momento.

## Crianças

O Elo não é voltado a crianças e não coleta dados de ninguém.

## Contato

Dúvidas: abra um chamado em
<https://github.com/Stringari-Consultoria/elo/issues>.
