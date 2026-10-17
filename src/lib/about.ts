/** The only external links the app may open (mirrored in src-tauri/capabilities/default.json). */
export const COFFEE_URL = "https://buymeacoffee.com/postmanpat";
export const PAYPAL_URL = "https://paypal.me/derekh100";
export const REPO_URL = "https://github.com/postmanpat-303/StreamWave";
export const ALLOWED_LINKS = [COFFEE_URL, PAYPAL_URL, REPO_URL] as const;
export type AllowedLink = (typeof ALLOWED_LINKS)[number];
export const AUTHOR = "Derek Heasley";
