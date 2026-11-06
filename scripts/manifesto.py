#!/usr/bin/env python3
"""Gera o latest.json da atualização automática (o CI assina em seguida).

Uso: scripts/manifesto.py <versão> <pasta com os pacotes> > latest.json
Exige os cinco pacotes da release; falta algum = erro (não publica pela metade).
"""
import hashlib
import json
import os
import sys

TIPOS = [
    ("windows-x64", "-windows-x64-setup.exe"),
    ("windows-x64-zip", "-windows-x64.zip"),
    ("macos", "-macos.dmg"),
    ("linux-x86_64", "-linux-x86_64.tar.gz"),
    ("linux-x86_64-deb", "_amd64.deb"),
]


def main() -> int:
    versao, pasta = sys.argv[1], sys.argv[2]
    assets = {}
    for nome in sorted(os.listdir(pasta)):
        # Os pacotes do servidor avulso (elo-server*) não são do app.
        if nome.lower().startswith("elo-server"):
            continue
        for chave, fim in TIPOS:
            if nome.endswith(fim) and versao in nome:
                caminho = os.path.join(pasta, nome)
                with open(caminho, "rb") as f:
                    dados = f.read()
                assets[chave] = {"name": nome, "sha256": hashlib.sha256(dados).hexdigest(), "size": len(dados)}
    faltando = [c for c, _ in TIPOS if c not in assets]
    if faltando:
        print(f"pacotes faltando para {versao}: {', '.join(faltando)}", file=sys.stderr)
        return 1
    manifesto = {
        "version": versao,
        "base_url": f"https://github.com/Stringari-Consultoria/elo/releases/download/v{versao}/",
        "assets": assets,
    }
    json.dump(manifesto, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
