#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Gabriel Stringari
# SPDX-License-Identifier: AGPL-3.0-only
# Procura cópias do Elo no GitHub: código com frases-canário (trechos bem
# específicos do Elo) fora do repositório oficial, e projetos que se
# apresentam como "by Stringari". Não altera nada; só lista.
#
# As frases ficam fora do repositório, em ~/.config/elo-vigia/canarios.txt
# (uma por linha; # comenta), para quem copia não saber quais apagar.
# Requer o gh autenticado (a busca de código do GitHub exige login).
# Uso: scripts/vigiar-copias.sh [arquivo-de-canários]
set -euo pipefail
LISTA="${1:-${XDG_CONFIG_HOME:-$HOME/.config}/elo-vigia/canarios.txt}"
OFICIAL="Stringari-Consultoria/elo"
[[ -f "$LISTA" ]] || { echo "sem lista de canários em $LISTA" >&2; exit 2; }

achados=0
busca() {  # busca <tipo: code|repositories> <consulta>
    gh api -X GET "search/$1" -f q="$2" -f per_page=30 2>/dev/null | python3 -c '
import json, sys
oficial = sys.argv[1]
for i in json.load(sys.stdin).get("items", []):
    repo = i.get("repository", i).get("full_name", "")
    if repo.lower() != oficial.lower():
        print(repo + "  " + i.get("html_url", ""))
' "$OFICIAL" || true
}

while IFS= read -r frase; do
    [[ -z "$frase" || "$frase" == \#* ]] && continue
    r="$(busca code "\"$frase\" -repo:$OFICIAL")"
    if [[ -n "$r" ]]; then
        echo "== \"$frase\""
        echo "$r"
        achados=$((achados + 1))
    fi
    sleep 7  # a busca de código aceita 10 consultas por minuto
done < "$LISTA"

r="$(busca repositories "\"by Stringari\" in:readme,description")"
if [[ -n "$r" ]]; then
    echo "== projetos que citam \"by Stringari\""
    echo "$r"
    achados=$((achados + 1))
fi

echo "frases com ocorrências fora do repositório oficial: $achados"
