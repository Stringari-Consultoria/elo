# Proveniência e licença

Como saber se um programa veio do Elo e se um build é o oficial. O Elo é
software livre sob a GNU AGPL-3.0, com os termos adicionais do arquivo
[NOTICE](../NOTICE): quem redistribui, modifica ou oferece o Elo pela rede
precisa manter o crédito "by Stringari" e oferecer o código-fonte.

## Em cada arquivo

Todo arquivo do repositório declara autoria e licença no padrão
[REUSE](https://reuse.software):

- arquivos de código têm um cabeçalho com `SPDX-FileCopyrightText` e
  `SPDX-License-Identifier: AGPL-3.0-only`;
- imagens, documentos e dados estão em [REUSE.toml](../REUSE.toml);
- os textos das licenças estão em [LICENSES/](../LICENSES).

Ferramentas de conformidade de licenças (ScanCode, FOSSA, Black Duck, `reuse
lint`) leem esses cabeçalhos: quem incorporar o Elo num produto recebe o aviso
de AGPL da própria ferramenta.

## Em cada executável

O `build.rs` do `elo-proto` grava no executável o commit e a data dele e se o
build saiu do CI do repositório oficial:

```
$ elo-server --version
elo-server 0.2.8 (commit 7f7d966e07de, 2026-11-17, build oficial)
código-fonte: https://github.com/Stringari-Consultoria/elo/tree/7f7d966e07de
Elo by Stringari (Stringari Consultoria & Tech). Software livre sob a GNU AGPL-3.0, ...
```

O app mostra o mesmo em Configurações → Sobre. "Build oficial" é só uma
indicação: a prova é o release do GitHub, cujo manifesto de atualização
(`latest.json`) é assinado com a chave Ed25519 da Stringari. Ninguém consegue
publicar uma atualização que o Elo oficial aceite.

## No servidor (AGPL, seção 13)

Quem usa um servidor do Elo pela rede tem direito ao código dele. Ao conectar,
o `elo-server` informa ao app a versão e o endereço do código-fonte daquela
versão (itens `versao=` e `fonte=` da lista de recursos do `Welcome`), e o app
mostra isso em Configurações → Sobre. O servidor também registra o endereço no
início do log. Uma versão modificada oferecida pela rede precisa apontar para o
próprio código.

## No protocolo

Estas constantes entram na derivação das chaves, nas identidades do SPAKE2 e
nos certificados. Um programa que converse com o Elo precisa usá-las
exatamente iguais, então a presença delas num app ou servidor compatível
indica que ele foi derivado do Elo ou o reimplementa a partir dele:

| Constante | Onde |
|---|---|
| `elo-v3` | sal do HKDF das chaves da sessão (`crates/proto/src/secure.rs`) |
| `viewer->host`, `host->viewer`, `sas`, `vinculo` | rótulos do HKDF |
| `confirm-a`, `confirm-b` | confirmação do SPAKE2 |
| `elo-visualizador:`, `elo-anfitriao:` | identidades do SPAKE2 |
| `elo-certificado-tecnico-v1`, `elo-sessao-tecnico-v1` | assinaturas dos técnicos (`cert.rs`) |
| `ELO2-` | prefixo dos códigos de convite |

## Para quem quiser usar o Elo

Pode, nos termos da licença: mantenha o crédito "by Stringari" na janela
principal e o aviso "baseado no Elo, de Stringari Consultoria & Tech" com o
link do repositório na tela Sobre, diga que a versão foi modificada e por quem,
e ofereça o código-fonte a quem recebe o programa ou o usa pela rede. Dúvidas:
abra um chamado em <https://github.com/Stringari-Consultoria/elo/issues>.
