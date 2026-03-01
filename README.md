# City Grow for Lively Wallpaper

![City Grow Preview](./assets/city-grow-gif.gif)

A procedurally generated visualization that grows and retracts in a continuous loop. Designed for use as an animated wallpaper with [Lively Wallpaper](https://github.com/rocksdanister/lively).

It is a Lively (Windows) port of the [KDE City Grow](https://github.com/HobbyBlobby/PlasmaWallpaper_CityGrow), which I found very pleasing.

It is fully hardware accelerated using Direct2D, so it should run smoothly even on lower-end hardware.

## Quick start

> As per [this](https://github.com/rocksdanister/lively/wiki/Differences-Between-Distributions), to run application wallpapers, you need the standalone version of Lively Wallpaper, not the Microsoft Store version.

To install the latest release, download the `.zip` bundle from the [releases page](https://github.com/Miosp/city_grow_lively/releases) and drag-and-drop it into the Lively Wallpaper application.

### Customization

To customize how the wallpaper works, open lively, click the three dots next to the City Grow wallpaper, and select `Open File Location`. This will open the folder where the wallpaper executable is located. You can edit the `city_grow.yaml` to change the configuration variables.

## Building

```bash
cargo build --release
```

The executable will be at `target/release/city_grow_rs.exe`.

## Running

Simply run the executable:

```bash
cargo run --release
```

## A short guide for developing Lively application wallpapers

The most crucial and tricky part of developing this wallpaper was figuring out that on newer Windows versions (> 11 24H2) the wallpaper compositor must be hardware accelerated, otherwise the window just gets killed immediately after launch. This means that GDI / GDI+ based wallpapers won't work, and you need to use a more modern option like DirectComposer (like here) or Windows.UI.Composition (maybe here in the future).
