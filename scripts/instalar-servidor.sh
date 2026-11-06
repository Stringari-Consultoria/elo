#!/usr/bin/env bash
# Instala o servidor do Elo como serviço do systemd (vai no .tar.gz do servidor).
#   sudo ./instalar.sh            instala (ou atualiza) e inicia
#   sudo ./instalar.sh --remover  para e remove (mantém os dados em /var/lib/elo-server)
set -euo pipefail

AQUI="$(cd "$(dirname "$0")" && pwd)"
BIN=/usr/local/bin/elo-server
UNIT=/etc/systemd/system/elo-server.service
PADRAO=/etc/default/elo-server

if [[ $EUID -ne 0 ]]; then
    echo "Rode com sudo: sudo $0 $*" >&2
    exit 1
fi
if [[ ! -d /run/systemd/system ]]; then
    echo "Este sistema não usa systemd. Rode o servidor direto, por exemplo:" >&2
    echo "  $AQUI/elo-server --listen 0.0.0.0:21116 --data /var/lib/elo-server --self-signed" >&2
    exit 1
fi

if [[ "${1:-}" == "--remover" ]]; then
    systemctl disable --now elo-server.service 2>/dev/null || true
    rm -f "$UNIT" "$BIN"
    systemctl daemon-reload
    echo "Servidor removido. Os dados ficaram em /var/lib/elo-server e as opções em $PADRAO."
    exit 0
fi

if [[ -x /usr/bin/elo-server ]] && dpkg -s elo-server >/dev/null 2>&1; then
    echo "O pacote .deb elo-server já está instalado: atualize por ele (sudo apt install ./elo-server_<versão>_<arq>.deb)." >&2
    exit 1
fi

install -m 755 "$AQUI/elo-server" "$BIN"
sed "s#/usr/bin/elo-server#$BIN#" "$AQUI/elo-server.service" > "$UNIT"
chmod 644 "$UNIT"
# As opções do usuário ficam (não sobrescreve numa atualização).
[[ -f "$PADRAO" ]] || install -m 644 "$AQUI/elo-server.default" "$PADRAO"
systemctl daemon-reload
systemctl enable elo-server.service >/dev/null
systemctl restart elo-server.service

FP=""
for _ in $(seq 20); do
    FP="$("$BIN" --impressao --data /var/lib/elo-server 2>/dev/null)" && break
    sleep 0.25
done
IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
echo
echo "Servidor do Elo rodando (serviço elo-server, porta 21116)."
echo "Nos apps, em Configurações → Servidor, desligue \"Rodar o servidor neste computador\" e use:"
echo "  Endereço:  ${IP:-<ip-deste-servidor>}:21116   (TLS ligado)"
[[ -n "$FP" ]] && echo "  Impressão digital:  $FP"
echo "Opções em $PADRAO (depois: sudo systemctl restart elo-server). Logs: journalctl -u elo-server"
echo "Libere a porta 21116/TCP no firewall, se houver."
