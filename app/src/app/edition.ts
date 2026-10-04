export type LauncherEdition = "classic" | "win11";

export const launcherEdition: LauncherEdition =
  import.meta.env.VITE_LAUNCHER_EDITION === "win11" ? "win11" : "classic";

export const isWindows11Edition = launcherEdition === "win11";
