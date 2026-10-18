/** The only external links the app may open (mirrored in src-tauri/capabilities/default.json). */
export const COFFEE_URL = "https://buymeacoffee.com/postmanpat";
export const PAYPAL_URL = "https://paypal.me/derekh100";
export const YOUTUBE_URL = "https://www.youtube.com/@whatgoesaround1551";
export const REPO_URL = "https://github.com/postmanpat-303/StreamWave";
/** Download page; on macOS updates are installed by hand from here (no in-app updater on Mac yet). */
export const RELEASES_URL = "https://github.com/postmanpat-303/StreamWave/releases/latest";
export const ALLOWED_LINKS = [COFFEE_URL, PAYPAL_URL, YOUTUBE_URL, REPO_URL, RELEASES_URL] as const;
export type AllowedLink = (typeof ALLOWED_LINKS)[number];
export const AUTHOR = "Del (postmanpat)";
