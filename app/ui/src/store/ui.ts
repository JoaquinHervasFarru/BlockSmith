import { create } from "zustand";

export type ThemePref = "system" | "light" | "dark";

const KEY = "blocksmith.theme";

function readTheme(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

function applyTheme(t: ThemePref) {
  const root = document.documentElement;
  if (t === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", t);
}

interface UiState {
  theme: ThemePref;
  setTheme: (t: ThemePref) => void;
}

export const useUi = create<UiState>((set) => ({
  theme: readTheme(),
  setTheme: (theme) => {
    applyTheme(theme);
    try {
      localStorage.setItem(KEY, theme);
    } catch {
      /* almacenamiento no disponible: el tema dura solo esta sesión */
    }
    set({ theme });
  },
}));

applyTheme(readTheme());
