# Elo — atalhos de build. `make` lista os comandos.
#
# Executáveis (build, run, install) são compilados no sistema, em target/host:
# dentro do VS Code em flatpak, os comandos saem do sandbox (flatpak-spawn
# --host). Um binário compilado no sandbox herda a glibc dele e pode não abrir
# no sistema. Testes e lint rodam onde você estiver, em target/.

CARGO   := $(if $(wildcard $(HOME)/.cargo/bin/cargo),$(HOME)/.cargo/bin/cargo,cargo)
HOST    := $(if $(wildcard /.flatpak-info),flatpak-spawn --host env -C $(CURDIR),)
HOSTDIR := target/host
ONHOST  := $(HOST) env CARGO_TARGET_DIR=$(HOSTDIR) $(CARGO)
DIST    := dist

PORTA ?= 21116
DADOS ?= dados-servidor

.DEFAULT_GOAL := help
.PHONY: help build run run-dev server server-tls install uninstall pacote-linux pacote-servidor pacote-macos pacote-windows test lint ci clean

help: ## Lista os comandos
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  make %-12s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Compila em release e copia os executáveis para dist/
	$(ONHOST) build --release --locked -p elo -p elo-server
	@mkdir -p $(DIST)
	@cp $(HOSTDIR)/release/elo $(HOSTDIR)/release/elo-server $(DIST)/
	@echo "Pronto: $(DIST)/elo e $(DIST)/elo-server"

run: ## Abre o app (release, com o servidor embutido)
	$(ONHOST) run --release -p elo

run-dev: ## Abre o app em modo debug, com configuração separada (.dev-config)
	$(HOST) env ELO_CONFIG_DIR=$(CURDIR)/.dev-config CARGO_TARGET_DIR=$(HOSTDIR) $(CARGO) run -p elo

server: ## Servidor avulso local sem TLS, para testes (PORTA=21116 DADOS=dados-servidor)
	$(ONHOST) run --release -p elo-server -- --listen 127.0.0.1:$(PORTA) --data $(DADOS) --no-tls

server-tls: ## Servidor avulso na rede com TLS autoassinado (imprime a impressão digital)
	$(ONHOST) run --release -p elo-server -- --listen 0.0.0.0:$(PORTA) --data $(DADOS) --self-signed

install: ## Compila e instala em ~/.local/bin com atalho no menu de aplicativos
	$(HOST) env CARGO_TARGET_DIR=$(HOSTDIR) scripts/instalar-linux.sh

uninstall: ## Remove a instalação (mantém as configurações)
	$(HOST) scripts/instalar-linux.sh --remover

pacote-linux: ## Instaladores Linux em dist/: .deb e .tar.gz
	$(HOST) env CARGO_TARGET_DIR=$(HOSTDIR) scripts/pacote-linux.sh

pacote-servidor: ## Servidor avulso para Linux em dist/: .deb (systemd) e .tar.gz
	$(HOST) env CARGO_TARGET_DIR=$(HOSTDIR) scripts/pacote-servidor.sh

pacote-macos: ## Instalador macOS em dist/: .dmg com Elo.app (só no macOS)
	scripts/pacote-macos.sh

pacote-windows: ## Instalador Windows em dist/: setup .exe e .zip (só no Windows)
	pwsh scripts/pacote-windows.ps1

test: ## Roda os testes (os de sessão usam o X11 de DISPLAY)
	$(CARGO) test --workspace

lint: ## Clippy como no CI
	$(CARGO) clippy --workspace --all-targets -- -D warnings

ci: lint test ## O mesmo que o CI roda

clean: ## Apaga builds e dist/
	$(CARGO) clean
	rm -rf $(DIST)
