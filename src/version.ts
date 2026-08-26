import packageMetadata from "../package.json";

/** The frontend and Tauri bundle both derive their displayed version from package.json. */
export const appVersion = packageMetadata.version;
