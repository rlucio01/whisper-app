import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "up_to_date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdaterInfo {
  status: UpdateStatus;
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  error: string | null;
}

const INITIAL: UpdaterInfo = {
  status: "idle",
  version: null,
  notes: null,
  downloaded: 0,
  total: null,
  error: null,
};

/** Hook compartilhado entre App.tsx (checagem silenciosa no boot + banner) e
 *  Settings.tsx (botão manual "Verificar agora"). Guarda o objeto `Update`
 *  retornado pelo plugin numa ref — ele carrega a URL/assinatura do
 *  instalador e é reusado no `installNow` sem precisar checar de novo. */
export function useUpdater() {
  const [info, setInfo] = useState<UpdaterInfo>(INITIAL);
  const updateRef = useRef<Update | null>(null);

  const checkNow = useCallback(async (opts?: { silent?: boolean }) => {
    const silent = opts?.silent ?? false;
    if (!silent) setInfo((i) => ({ ...i, status: "checking", error: null }));
    try {
      const update = await check();
      updateRef.current = update;
      if (update) {
        setInfo({
          status: "available",
          version: update.version,
          notes: update.body ?? null,
          downloaded: 0,
          total: null,
          error: null,
        });
        invoke("set_tray_update_available", { version: update.version }).catch(() => {});
      } else {
        invoke("set_tray_update_available", { version: null }).catch(() => {});
        if (!silent) {
          setInfo({ ...INITIAL, status: "up_to_date" });
        }
      }
    } catch (e) {
      // Checagem silenciosa (boot) falha em silêncio — sem internet, sem
      // repo acessível etc. não deve incomodar o usuário sem ele pedir.
      if (!silent) setInfo({ ...INITIAL, status: "error", error: String(e) });
    }
  }, []);

  const installNow = useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;
    setInfo((i) => ({ ...i, status: "downloading", downloaded: 0, total: null }));
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setInfo((i) => ({ ...i, total: event.data.contentLength ?? null }));
        } else if (event.event === "Progress") {
          setInfo((i) => ({
            ...i,
            downloaded: i.downloaded + event.data.chunkLength,
          }));
        } else if (event.event === "Finished") {
          setInfo((i) => ({ ...i, status: "installing" }));
        }
      });
      await relaunch();
    } catch (e) {
      setInfo((i) => ({ ...i, status: "error", error: String(e) }));
    }
  }, []);

  return { info, checkNow, installNow };
}
