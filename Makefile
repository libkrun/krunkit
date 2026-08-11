OS = $(shell uname -s)
KRUNKIT_RELEASE = target/release/krunkit
KRUNKIT_DEBUG = target/debug/krunkit
LIBKRUN = libkrun.1.dylib
FIRMWARE = edk2/KRUN_EFI.silent.fd

PREFIX ?= /usr/local
export PREFIX

.PHONY: install clean $(KRUNKIT_RELEASE) $(KRUNKIT_DEBUG)

all: $(KRUNKIT_RELEASE)

debug: $(KRUNKIT_DEBUG)

$(KRUNKIT_RELEASE):
	cargo build --release
ifeq ($(OS),Darwin)
	install_name_tool -change $(LIBKRUN) $(PREFIX)/lib/$(LIBKRUN) -add_rpath $(PREFIX)/lib $@
	codesign --entitlements krunkit.entitlements --force -s - $@
endif

$(KRUNKIT_DEBUG):
	cargo build --debug

install: $(KRUNKIT_RELEASE)
	install -d $(DESTDIR)$(PREFIX)/bin
	install -m 755 $(KRUNKIT_RELEASE) $(DESTDIR)$(PREFIX)/bin
	install -d $(DESTDIR)$(PREFIX)/share/krunkit
	install -m 644 $(FIRMWARE) $(DESTDIR)$(PREFIX)/share/krunkit

clean:
	cargo clean
