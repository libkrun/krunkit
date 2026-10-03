# krunkit

`krunkit` is a tool to launch configurable virtual machines using the [libkrun](https://github.com/containers/libkrun) platform.

> [!IMPORTANT]
> The established krunkit path supports macOS 14 or newer. Experimental Windows support uses libkrun 2.0 and WHP.

## Installation

> [!IMPORTANT]
> If you've ever installed krunkit from the old tap `slp/krunkit`, to upgrade to the latest version you'll need to follow the [Removing the old Homebrew tap](#removing-the-old-homebrew-tap) instructions first, and then the ones in the [Installing from Homebrew](#installing-from-homebrew) section.

### Removing the old Homebrew tap

The `slp/krunkit` tap is now deprecated. If you've ever installed `krunkit` from it, you'll need to remove all packages from that tap and then the tap itself:

```
brew list --full-name | grep "^slp/krunkit/" | xargs brew uninstall
brew untap slp/krunkit
```

### Installing from Homebrew

`krunkit` relies on `libkrun`. We provide a Homebrew repository to install `krunkit` and all of its dependencies, installable with:

```
brew tap slp/krun
brew install krunkit
```

## Building from source

As noted above, `krunkit` relies on `libkrun`. Ensure that is installed on your system.

Build and install using default `PREFIX` (`/usr/local`):

```
make
sudo make install
```

On Windows, `LIBKRUN_LIB_DIR` must point to the directory containing `krun.dll.lib` when invoking Cargo.
Place the matching `krun.dll` beside `krunkit.exe` to run it. For local development, `build.ps1` builds a
libkrun 2.0 tree and copies its DLL beside the executable:

```powershell
.\build.ps1 -LibkrunSource C:\src\libkrun
```

To build with `libkrun` from *Homebrew* or *MacPorts* use the appropriate `PREFIX`:

```
make PREFIX=/opt/homebrew
sudo make install PREFIX=/opt/homebrew
```

## Usage

See [`docs/usage.md`](./docs/usage.md).

License: Apache-2.0
