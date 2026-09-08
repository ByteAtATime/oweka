# oweka

oweka (from [toki pona](https://tokipona.net) _o weka_, "be gone!") is an extremely fast and lightweight TUI that helps you get rid of unwanted artifact directories, like `node_modules`, `target`, `.venv`, etc.

![Recording of oweka being used](https://raw.githubusercontent.com/ByteAtATime/oweka/main/.github/demo.gif)

## Installation

If you want to try out oweka without installing, you can simply run it with `npx`:

```bash
$ npx oweka
$ # or
$ pnpx oweka
$ bunx oweka
```

Alternatively, you can install it [from the releases page](https://github.com/ByteAtATime/oweka/releases).

## Features

- **Performant**. oweka is designed to scan your disk for relevant directories as fast as possible. On most systems, it should reasonably complete in just a few seconds.
- **Accurate**. Detection of artifact directories are done in code to ensure they are likely to be relevant. For example, oweka only flags Python venvs if they contain a `pyvenv.cfg`; similarly, Rust's `target` checks for a corresponding `Cargo.toml`, etc.
- **Extensible**. It is extremely easy to add a new matcher. If you'd like to request one, [feel free to open an issue!](https://github.com/ByteAtATime/oweka/issues)

## Usage

Simply run your installed binary, e.g. `oweka` or `pnpx oweka`. By default, it will recursively scan your current directory; to scan a different one, specify it as the path, e.g. `oweka ~/projects`.

Move your selection by pressing the <kbd>↓</kbd><kbd>↑</kbd>/<kbd>j</kbd><kbd>k</kbd> keys, and press <kbd>Space</kbd> or <kbd>Enter</kbd> to delete the highlighted folder.

Press <kbd>o</kbd> to open the highlighted folder in your file manager.

**Warning:** pressing <kbd>Space</kbd> or <kbd>Enter</kbd> will **irreversibly** delete the folder **without confirmation**.

Yellow paths indicate that they may be used by the system, e.g. those in `.local`. Take caution when deleting these!

## Acknowledgements

This project takes heavy inspiration from [NPKILL](https://github.com/voidcosmos/npkill), a similar project aimed at node_modules.

The directory traversal logic is largely taken from [dua-cli](https://github.com/Byron/dua-cli), although I have rewritten most of it for speed.

The ethos of this project is also inspired by programs like [ripgrep](https://github.com/BurntSushi/ripgrep) and [fd](https://github.com/sharkdp/fd)!

Built with my own two paws!
