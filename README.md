[![Linux build](https://github.com/kamiyaa/joshuto/actions/workflows/rust-linux-main.yml/badge.svg)](https://github.com/kamiyaa/joshuto/actions/workflows/rust-linux-main.yml?branch=main)

[![MacOS build](https://github.com/kamiyaa/joshuto/actions/workflows/rust-macos-main.yml/badge.svg)](https://github.com/kamiyaa/joshuto/actions/workflows/rust-macos-main.yml?branch=main)

# joshuto

[ranger](https://github.com/ranger/ranger)-like terminal file manager written in Rust.

![Alt text](screenshot.png?raw=true "joshuto")

## Dependencies

- [cargo](https://github.com/rust-lang/cargo/) >= 1.90
- [rustc](https://www.rust-lang.org/) >= 1.90
- xsel/xclip/wl-clipboard (optional, for clipboard support)
- fzf (optional)
- zoxide (optional)

Also see [Cargo.toml](Cargo.toml)

## Building

```
~$ cargo build
```

## Installation

#### For single user

```
~$ cargo install --path=. --force
```

#### For single user with cargo

```
~$ cargo install --git https://github.com/kamiyaa/joshuto.git --force
```

#### System wide

```
~# cargo install --path=. --force --root=/usr/local     # /usr also works
```

#### From pre-compiled binary

Dependencies:
- curl
- openssl

##### Latest release

Installs the latest version using the default installation path (_$HOME/.local/bin/_).

``` bash
~$ bash <(curl -s https://raw.githubusercontent.com/kamiyaa/joshuto/master/utils/install.sh)
```

##### Custom Installation path

Allows you to install Joshuto to a custom directory by setting the INSTALL_PREFIX variable.

``` bash
~$ INSTALL_PREFIX="$HOME" bash <(curl -s https://raw.githubusercontent.com/kamiyaa/joshuto/master/utils/install.sh)
```

##### System wide

``` bash
~# INSTALL_PREFIX="/usr/local/bin" bash <(curl -s https://raw.githubusercontent.com/kamiyaa/joshuto/master/utils/install.sh)
```

##### Specific release

Installs a specific release version of Joshuto by the desired version number.

``` bash
~$ RELEASE_VER='v0.9.4' bash <(curl -s https://raw.githubusercontent.com/kamiyaa/joshuto/master/utils/install.sh)
```

#### Packaging status

##### Fedora ([COPR](https://copr.fedorainfracloud.org/coprs/atim/joshuto/))

```
sudo dnf copr enable atim/joshuto -y
sudo dnf install joshuto
```

##### Arch ([AUR](https://aur.archlinux.org))

- [release](https://aur.archlinux.org/packages/joshuto)

```
[yay/paru] -S joshuto
```

- [build from source](https://aur.archlinux.org/packages/joshuto-git)

```
[yay/paru] -S joshuto-git
```

##### Arch ([archlinuxcn](https://github.com/archlinuxcn/repo/))

- [stable version (x86_64)](https://github.com/archlinuxcn/repo/tree/master/archlinuxcn/joshuto)
- [stable version (aarch64)](https://github.com/archlinuxcn/repo/tree/master/alarmcn/joshuto)

```
[yay/paru] -S joshuto
```

- [latest git version (x86_64)](https://github.com/archlinuxcn/repo/tree/master/archlinuxcn/joshuto-git)
- [latest git version (aarch64)](https://github.com/archlinuxcn/repo/tree/master/alarmcn/joshuto-git)

```
[yay/paru] -S joshuto-git
```

##### Gentoo ([gentoo-zh](https://github.com/microcai/gentoo-zh/tree/master/app-misc/joshuto))

```
sudo eselect repository enable gentoo-zh
sudo emerge -av app-misc/joshuto
```

##### NixOS

> Here's an example of using it in a nixos configuration

```nix
{
  description = "My configuration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    joshuto.url = "github:kamiyaa/joshuto";
  };

  outputs = { nixpkgs, joshuto, ... }:
    {
      nixosConfigurations = {
        hostname = nixpkgs.lib.nixosSystem
          {
            system = "x86_64-linux";
            modules = [
              {
                nixpkgs.overlays = [ joshuto.overlays.default ];
                environment.systemPackages = with pkgs;[
                  joshuto
                ];
              }
            ];
          };
      };
    };
}
```

> Temporary run, not installed on the system

```sh
nix run github:kamiyaa/joshuto
```

##### MacOS ([MacPorts](https://ports.macports.org/port/joshuto/details/))

```
sudo port install joshuto
```

##### MacOS/Linux [Homebrew](https://brew.sh/)

```
brew install joshuto
```

## Usage

```
~ $ joshuto
```

#### Navigation
- Move up: `arrow_up` or `k`
- Move down: `arrow_down` or `j`
- Move to parent directory: `arrow_left` or `h`
- Open file or directory: `arrow_right` or `l`
- Go to the top: `home` or `g g`
- Go to the bottom: `end` or `G`
- Page up: `page_up` or `ctrl+u`
- Page down: `page_down` or `ctrl+d`

#### Tab Management
- Open a new tab: `ctrl+t`
- Open a new tab with current directory: `T`
- Close the current tab: `W` or `ctrl+w`
- Switch to next tab: `\t`
- Switch to previous tab: `backtab`

#### File Operations
- Rename file: `a` to append or `A` to prepend
- Delete file: `delete` or `d d`
- Cut file: `d d`
- Copy file: `y y`
- Paste file: `p p`
- Paste file with overwrite: `p o`
- Symlink files: `p l` for absolute path, `p L` for relative path

#### Miscellaneous
- Toggle hidden files: `z h`
- Reload directory list: `R`
- Change directory: `c d`
- Show tasks: `w`
- Set mode: `=`
- Enter command mode: `:`

See [docs#quit](/docs/configuration/keymap.toml.md#quit-quit-joshuto) for exiting into current directory
and other usages

## Configuration

Check out [docs](/docs) for details and [config](/config) for examples

#### [joshuto.toml](/config/joshuto.toml)

- general configurations

#### [keymap.toml](/config/keymap.toml)

- for keybindings

#### [mimetype.toml](/config/mimetype.toml)

- for opening files with applications

#### [theme.toml](/config/theme.toml)

- color customizations

#### [bookmarks.toml](/config/bookmarks.toml)

- bookmarks

## Contributing

See [docs](/docs)

## Bugs/Feature Request

Please create an issue :)

## Differences from upstream (kamiyaa/main)

This branch (`lali-joshuto-new`) builds on upstream joshuto with a focus on
**file-operation UX** and **directory-listing performance**.

### Selection & operation highlights

- Distinct, themeable colors per pending file operation. A new `[mark]` table in
  [`config/theme.toml`](/config/theme.toml) lets you color entries that are
  queued for a cut, copy, or symlink operation separately from the normal
  selection:
  - `mark.cut` &mdash; red (default)
  - `mark.copy` &mdash; orange (`rgb(235, 104, 65)`)
  - `mark.symlink` &mdash; pink (`rgb(254, 132, 172)`)
- The default selection colors were also tweaked: `[selection]` is now
  `light_green` and `[visual_mode_selection]` is `light_yellow`.
- Cut/copy/symlink now **mark the affected entries across all tabs** (stored in
  the directory history), so the pending operation stays visible wherever you
  navigate &mdash; not just in the tab where it was initiated.

### Cancelling file operations

- New `cancel_file_operation` command (bound to `p c` by default) clears the
  pending cut/copy/symlink and removes all of its visual marks. See
  [`docs/configuration/keymap.toml.md`](/docs/configuration/keymap.toml.md).

### Performance improvements (directory listing & previews)

- **Background directory loading:** directory listings are read on worker
  threads (`LoadDirectory` events) so the UI stays responsive; unchanged
  ancestor listings are reused from the cache instead of being re-read.
- **Fewer stat syscalls:** sorting uses already-cached entry metadata, and the
  redundant follow-`stat` for non-symlinks was removed (directory loads are
  noticeably faster on large directories).
- **Bounded worker pool + cancellation tokens** for directory and preview tasks,
  so rapid cursor movement cancels in-flight work instead of letting stale
  results pile up. Generation tracking prevents a late load from overwriting a
  newer listing.
- **Faster previews:** a directory preview reads only the first 200 entries for
  an instant first paint, then keeps reading the rest on the background thread
  and replaces the partial listing with the complete, fully-counted one (no
  manual refresh needed). The script-preview channel is drained so only the
  latest entry is rendered.
- A small bounded-concurrency `semaphore` utility was added to cap parallel work.

### Other

- New `restart` command.
- State is persisted when opening a new tab.
- Dependency and build updates (e.g. `ratatui-image` 1.0.5, `zigbuild`/musl
  build tweaks).

## Features

- Tabs
- Devicons
- Fuzzy search via [fzf](https://github.com/junegunn/fzf)
- Ctrl/Shift/Alt support
- Bulk rename
- File previews
  - See [Image previews](/docs/image_previews) for more details
- Exit to current directory
- Asynch File IO (cut/copy/paste)
- Custom colors/theme
- Line numbers
  - Jump to number
- File chooser
- Trash support

## TODOs

- [x] Built-in command line
  - Mostly working
  - Currently implementation is kind of janky
  - [ ] Tab autocomplete (in progress)
