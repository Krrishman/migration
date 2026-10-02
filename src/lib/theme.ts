export type ThemePref = "system" | "light" | "dark";

const KEY = "ma-theme";

export function loadTheme(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(pref: ThemePref) {
  const dark = pref === "dark" || (pref === "system" && window.matchMedia?.("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", !!dark);
  try {
    localStorage.setItem(KEY, pref);
  } catch {
    /* storage unavailable */
  }
}
