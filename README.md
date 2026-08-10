# autoAppImage

📦 An automatic AppImage manager for Linux.

Run it once — it finds new `.AppImage` files,
moves them somewhere sensible, extracts the icon and name from inside the
AppImage, and creates a proper desktop shortcut. Delete the AppImage later,
and it cleans up the shortcut and icon it created for it. No more manual
`chmod +x`, hand-written `.desktop` files, or broken icons.

## Features

- 🚚 **Moves** every `.AppImage` file from `~/Downloads` into `~/Applications`
- ⚡ **Detects new** AppImages in `~/Applications` and creates shortcuts for them
- 🖼️ **Extracts the icon** straight from the AppImage (prioritizing SVG /
  scalable icons, falling back to the largest available raster resolution)
- 🔧 **Sets the executable bit** automatically if it's missing
- 🗑️ **Cleans up after itself** — if you delete an AppImage, the shortcut
  and icon it created are removed automatically
- 🛡️ Only touches what it created itself (tags its own `.desktop` files
  with a marker, and leaves everything else alone)

## Requirements

- Linux (uses `.local/share/applications`, `PermissionsExt`, etc.)
- [Rust](https://www.rust-lang.org/tools/install) to build

## Build

```bash
git clone https://github.com/<your_username>/autoAppImage.git
cd autoAppImage
cargo build --release
```

The compiled binary will be at `target/release/autoAppImage`.

## Usage

Just drop `.AppImage` files into `~/Downloads` like you normally would when
downloading them, then run:

```bash
./target/release/autoAppImage
```

It will:
1. Move the file from `Downloads` into `~/Applications`.
2. Create an application menu shortcut with the correct icon and name.

It's convenient to put the binary somewhere on your `$PATH` (e.g.
`~/.local/bin`) so you can just run it by name.


## How it works

- Icons are searched for inside the extracted `squashfs-root` of the
  AppImage: first by matching the name from the `.desktop` file's `Icon=`
  entry (preferring SVG/`scalable` icons, then the largest raster
  resolution available), falling back to `.DirIcon` if nothing matches.
- Every `.desktop` file created by the script is tagged with a marker line
  (`X-AppImage-CreatedBy=autoAppImage`), which is how it recognizes and
  manages "its own" shortcuts without touching anything created manually
  or by another program.

## License

MIT
