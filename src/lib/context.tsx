import { createContext, useContext } from "react";
import type { Backend } from "./api";
import type { AppStatus } from "./types";

export interface AppContextValue {
  backend: Backend;
  status: AppStatus;
  refreshStatus: () => Promise<void>;
  notify: (message: string, tone?: "ok" | "danger" | "neutral") => void;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const v = useContext(AppContext);
  if (!v) throw new Error("useApp outside provider");
  return v;
}
