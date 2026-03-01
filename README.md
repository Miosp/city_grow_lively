# City Grow for Lively Wallpaper

![City Grow Preview](./assets/city-grow-gif.gif)

A procedurally generated visualization that grows and retracts in a continuous loop. Designed for use as an animated wallpaper with [Lively Wallpaper](https://github.com/rocksdanister/lively).

It is a Lively (Windows) port of the [KDE City Grow](https://github.com/HobbyBlobby/PlasmaWallpaper_CityGrow), which I found very pleasing.

## Quick start

To install the latest release, download the `.zip` bundle from the [releases page](https://github.com/Miosp/city_grow_lively/releases) and drag-and-drop it into the Lively Wallpaper application.

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
