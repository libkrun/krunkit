# krunkit

`krunkit` is a tool to launch configurable virtual machines using the [libkrun](https://github.com/containers/libkrun) platform.

> [!IMPORTANT]
> krunkit is only supported on hosts running macOS 14 or newer.

## Installation

> [!IMPORTANT]
> If you've ever installed krunkit from the old tap `slp/krunkit`, to upgrade to the latest version you'll need to follow the [Removing the old Homebrew tap](#removing-the-old-homebrew-tap) instructions first, and then the ones in the [Installing from Homebrew](#installing-from-homebrew) section.

### Removing the old Homebrew tap

The `slp/krunkit` tap is now deprecated. If you've ever installed `krunkit` from it, you'll need to remove all packages from that tap and then the tap itself:

```shell
brew list --full-name | grep "^slp/krunkit/" | xargs brew uninstall
brew untap slp/krunkit
```

### Installing from Homebrew

`krunkit` relies on `libkrun`. We provide a Homebrew repository to install `krunkit` and all of its dependencies, installable with:

```shell
brew tap libkrun/krun
brew trust libkrun/krun
brew install krunkit
```

## Building from source

As noted above, `krunkit` relies on `libkrun`. Ensure that is installed on your system.

Build and install using default `PREFIX` (`/usr/local`):

```shell
make
sudo make install
```

To build with `libkrun` from *Homebrew* or *MacPorts* use the appropriate `PREFIX`:

```shell
make PREFIX=/opt/homebrew
sudo make install PREFIX=/opt/homebrew
```

## Usage

See [`docs/usage.md`](./docs/usage.md).

License: Apache-2.0
