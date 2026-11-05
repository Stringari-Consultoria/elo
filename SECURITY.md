# Segurança

O Elo dá acesso total a um computador. Levamos falhas de segurança a sério.

## Como relatar uma vulnerabilidade

**Não abra uma issue pública.** Use o relato privado do GitHub:
[**Relatar uma vulnerabilidade**](https://github.com/Stringari-Consultoria/elo/security/advisories/new)
(aba *Security* do repositório).

Inclua, se puder:

- o que a falha permite (por exemplo: acessar sem senha, ler a sessão, derrubar o servidor);
- a versão ou o commit afetado;
- os passos para reproduzir ou uma prova de conceito.

Vamos confirmar o recebimento, combinar com você um prazo para a correção e
dar o crédito no aviso de segurança, se você quiser.

## Escopo

Interessam principalmente:

- o aperto de mão SPAKE2 e o canal cifrado (`crates/proto/src/secure.rs`);
- autenticação, senhas e o bloqueio após tentativas erradas;
- o servidor (`apps/elo-server`): pareamento de sessões, limites de
  tentativas, TLS e WebSocket;
- qualquer forma de controlar, ver ou receber arquivos de um computador sem a
  permissão de quem está nele.

## Versões suportadas

O projeto está em alfa: só a versão mais recente recebe correções.
