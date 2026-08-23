import { useState } from "react";
import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary, LauncherErrorDto } from "../../app/types";

export function OfflineLogin({ api = launcherApi, onAuthenticated }: { api?: LauncherApi; onAuthenticated?: (account: AccountSummary) => void }) {
  const [name, setName] = useState("Player");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  async function start() {
    setBusy(true); setError(undefined);
    try { onAuthenticated?.(await api.createOfflineAccount(name)); }
    catch (value) { setError((value as Partial<LauncherErrorDto>)?.message ?? "Не удалось создать локальный профиль."); }
    finally { setBusy(false); }
  }
  return <div className="offline-login">
    <label htmlFor="offline-name">Локальное имя игрока</label>
    <div><input id="offline-name" maxLength={16} value={name} onChange={(e) => setName(e.target.value)} />
    <button disabled={busy} onClick={() => void start()} type="button">{busy ? "Создаём…" : "Играть офлайн"}</button></div>
    <small>Только одиночная игра и offline-mode серверы. Лицензия Microsoft не подтверждается.</small>
    {error ? <p role="alert">{error}</p> : null}
  </div>;
}
