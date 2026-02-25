# City Grow

A procedurally generated visualization that grows and retracts in a continuous loop. Designed for use as an animated wallpaper with [Lively Wallpaper](https://github.com/rocksdanister/lively).

It is a Lively (Windows) port of the [KDE City Grow](https://github.com/HobbyBlobby/PlasmaWallpaper_CityGrow), which I found very pleasing.

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

### Using with Lively Wallpaper

1. Build the release executable
2. Open Lively Wallpaper
3. Click "Open" and select the `city_grow_rs.exe` file
4. The animation will run as your desktop wallpaper
