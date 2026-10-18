# App icons

`icon.ico` is hand-built: 256 px is PNG, while 128/64/48/32/24/16 px are 32-bit BMP. Windows Explorer and Edge
often can't render PNG frames at small sizes and show the generic installer icon instead.

Don't regenerate it with `npx tauri icon` (that writes an all-PNG `.ico`). If you regenerate the PNGs,
restore `icon.ico` afterwards. `src/lib/icon.test.ts` fails if the small sizes aren't BMP.

`icon.ico` is used for the app exe, the installer and uninstaller, and the `streamwave.ico` that
Start-menu and desktop shortcuts point at.
