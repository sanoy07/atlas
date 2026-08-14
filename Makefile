# Atlas — local developer knowledge engine
PREFIX  ?= $(HOME)/.local
BINDIR  ?= $(PREFIX)/bin
CARGO   ?= cargo
ATLAS   := target/release/atlas

.PHONY: all build release test install uninstall status doctor clean help

all: release

help:
	@echo "make build     - debug build"
	@echo "make release   - release binary"
	@echo "make test      - workspace tests"
	@echo "make install   - install atlas to $(BINDIR)"
	@echo "make uninstall - remove $(BINDIR)/atlas"
	@echo "make status    - run atlas status after install"
	@echo "make doctor    - alias for status"

build:
	$(CARGO) build -p atlas

release:
	$(CARGO) build --release -p atlas

test:
	$(CARGO) test --workspace

install: release
	mkdir -p "$(BINDIR)"
	install -m 755 "$(ATLAS)" "$(BINDIR)/atlas"
	@echo ""
	@echo "Installed: $(BINDIR)/atlas"
	@echo "Ensure $(BINDIR) is on PATH, then run:  atlas status"

uninstall:
	rm -f "$(BINDIR)/atlas"
	@echo "Removed $(BINDIR)/atlas"

status doctor: install
	"$(BINDIR)/atlas" status

clean:
	$(CARGO) clean
