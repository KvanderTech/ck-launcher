import { useEffect, useRef, useState } from "react";
import { SkinViewer } from "skinview3d";

import type { AppApi } from "../../app/tauri";
import type { AccountSummary, OfflineSkin } from "../../app/types";

export function SkinsPage({ api, account }: { api: AppApi; account: AccountSummary }) {
  const [skins, setSkins] = useState<OfflineSkin[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const active = skins.find((skin) => skin.isActive) ?? skins[0];

  useEffect(() => {
    void api.listOfflineSkins(account.id).then(setSkins, () => setError("Не удалось открыть локальную галерею."));
  }, [account.id, api]);

  async function addSkin() {
    setBusy(true); setError(undefined);
    try {
      const skin = await api.addOfflineSkin(account.id);
      if (skin) setSkins((current) => [...current.map((item) => ({ ...item, isActive: false })), skin]);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  }

  async function selectSkin(skinId: string) {
    try {
      await api.selectOfflineSkin(account.id, skinId);
      setSkins((current) => current.map((item) => ({ ...item, isActive: item.id === skinId })));
    } catch (reason) { setError(errorMessage(reason)); }
  }

  return (
    <section className="skins-page">
      <div className="page-heading"><span className="eyebrow">Локальный профиль</span><h1>Скины и плащи</h1><p>Скины хранятся только на этом компьютере для профиля {account.minecraftName}.</p></div>
      <div className="offline-skin-layout">
        <section className="skin-viewer-card">
          {active ? <SkinCanvas skin={active.dataUrl} /> : <div className="skin-empty">Добавьте PNG-скин 64×64 или 64×32</div>}
          <strong>{active?.name ?? account.minecraftName}</strong>
          <small>Перетаскивайте модель для вращения</small>
        </section>
        <section className="saved-skins-card">
          <div className="content-section-title"><div><h2>Сохранённые скины</h2><p>Доступны без входа Microsoft.</p></div><button disabled={busy} onClick={() => void addSkin()} type="button">{busy ? "Добавление…" : "+ Добавить PNG"}</button></div>
          {error ? <p className="content-error" role="alert">{error}</p> : null}
          <div className="saved-skin-grid">
            {skins.map((skin) => <button className={skin.isActive ? "saved-skin active" : "saved-skin"} key={skin.id} onClick={() => void selectSkin(skin.id)} type="button"><SkinCanvas skin={skin.dataUrl} compact /><span>{skin.name}</span></button>)}
          </div>
          <div className="cape-note"><strong>Плащи</strong><p>Локальные плащи будут подключены через клиентский мод; официальный плащ требует Minecraft Services.</p></div>
        </section>
      </div>
    </section>
  );
}

function SkinCanvas({ skin, compact = false }: { skin: string; compact?: boolean }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    if (!canvas.current) return;
    const viewer = new SkinViewer({ canvas: canvas.current, width: compact ? 128 : 330, height: compact ? 150 : 390, skin });
    viewer.autoRotate = !compact; viewer.autoRotateSpeed = 0.7; viewer.zoom = compact ? 0.72 : 0.82;
    return () => viewer.dispose();
  }, [compact, skin]);
  return <canvas className={compact ? "skin-canvas compact" : "skin-canvas"} ref={canvas} />;
}

function errorMessage(error: unknown) {
  if (typeof error === "object" && error && "message" in error && typeof error.message === "string") return error.message;
  return "Операция со скином не выполнена.";
}
