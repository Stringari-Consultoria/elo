# Janela do .dmg (dmgbuild): fundo com a seta, Elo à esquerda e o atalho
# para Aplicativos à direita. Usado por scripts/pacote-macos.sh, que passa
# -D app=<Elo.app> -D fundo=<dmg-fundo.png> -D icone=<Elo.icns>.
# A imagem de fundo tem 660x400 (e @2x); os ícones ficam no centro dos círculos.
import os.path

app = defines["app"]  # noqa: F821 (definido pelo dmgbuild)
fundo = defines["fundo"]  # noqa: F821
icone = defines.get("icone")  # noqa: F821

format = "ULFO"  # lzfse: menor que UDZO; macOS 10.11+
files = [app]
symlinks = {"Aplicativos": "/Applications"}
if icone:
    icon = icone

background = fundo
# Altura um pouco maior que a imagem: a barra de título ocupa parte da janela.
window_rect = ((200, 140), (660, 428))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False

icon_size = 128
text_size = 13
arrange_by = None
icon_locations = {
    os.path.basename(app): (165, 196),
    "Aplicativos": (495, 196),
}
