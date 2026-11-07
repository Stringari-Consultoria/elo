# Contribuindo com o Elo

Obrigado pelo interesse! Correções, testes em outras plataformas e novas
funcionalidades são bem-vindos.

## Antes de começar

- **Bugs:** abra uma issue com o sistema (e a versão), o que você fez, o que
  esperava e o que aconteceu. Logs do terminal ajudam muito.
- **Funcionalidades grandes:** abra uma issue para conversar antes de escrever
  o código. Assim ninguém perde tempo com algo que não vai entrar.
- **Vulnerabilidades:** não abra issue pública. Veja o [SECURITY.md](SECURITY.md).

## Ambiente

Instale o Rust 1.88+ e as dependências listadas no README, em
[Desenvolvimento](README.md#desenvolvimento). Depois:

```sh
make run-dev   # abre o app com configuração separada (.dev-config/)
make ci        # clippy e testes, igual ao CI
```

Para testar uma conexão entre duas instâncias na mesma máquina, rode a segunda
com outra pasta de configuração:

```sh
ELO_CONFIG_DIR=/tmp/elo-b cargo run -p elo
```

Na segunda instância, desligue o servidor local e aponte para
`127.0.0.1:21116`, com a impressão digital mostrada nas configurações da
primeira.

## Pull requests

- Um assunto por PR, com descrição do que muda e de como você testou.
- `make ci` precisa passar: `cargo clippy --workspace --all-targets -- -D warnings`
  e `cargo test --workspace`.
- Mudanças no protocolo (`crates/proto`) precisam de teste e, se quebrarem
  compatibilidade, de aumentar o `PROTOCOL_VERSION`.
- Mudanças em criptografia, autenticação ou no servidor recebem revisão mais
  cuidadosa. Explique a motivação e o modelo de ameaça no PR.
- O projeto não usa `rustfmt`: siga o estilo do arquivo que você está editando.
- Textos da interface e mensagens de erro são em português do Brasil.

## Licença das contribuições

Ao enviar uma contribuição, você concorda que ela seja licenciada sob a AGPL 3.0
com os termos adicionais do [NOTICE](NOTICE), como o resto do projeto, e que a
Stringari Consultoria & Tech também possa distribuí-la sob outras licenças.
