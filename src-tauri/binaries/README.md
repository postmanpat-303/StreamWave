FFmpeg sidecars for the Windows installer go here (not committed):

    ffmpeg-x86_64-pc-windows-msvc.exe
    ffprobe-x86_64-pc-windows-msvc.exe

Get them from the LGPL build at https://github.com/BtbN/FFmpeg-Builds/releases
(`ffmpeg-n8.1-latest-win64-lgpl-8.1.zip`, `bin/` folder) and build with
`npm run tauri build -- --config src-tauri/tauri.windows-installer.conf.json`.
CI does this automatically (.github/workflows/release.yml).
