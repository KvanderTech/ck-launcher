import { useEffect, useRef, useState } from "react";
import { SkinViewer } from "skinview3d";

import type { AppApi } from "../../app/tauri";
import type { AccountSummary, MinecraftCosmetics, OfflineSkin } from "../../app/types";

interface SkinsPageProps {
  api: AppApi;
  account: AccountSummary;
  skins: OfflineSkin[];
  cosmetics?: MinecraftCosmetics;
  error?: string;
  loading: boolean;
  onSkinsChange(skins: OfflineSkin[]): void;
  onCosmeticsChange(cosmetics: MinecraftCosmetics): void;
  onRefresh(): void;
}

export function SkinsPage({ api, account, skins, cosmetics, error: loadError, loading, onSkinsChange, onCosmeticsChange, onRefresh }: SkinsPageProps) {
  const [selectedId, setSelectedId] = useState<string | null>();
  const [variant, setVariant] = useState<"classic" | "slim">("classic");
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const selected = selectedId === null ? undefined : skins.find((skin) => skin.id === selectedId) ?? skins.find((skin) => skin.isActive) ?? skins[0];
  const licensedSkin = cosmetics?.skins.find((skin) => skin.state === "ACTIVE") ?? cosmetics?.skins[0];
  const previewSkin = selected?.dataUrl ?? licensedSkin?.url;

  useEffect(() => { setSelectedId(undefined); }, [account.id]);
  useEffect(() => {
    const activeVariant = cosmetics?.skins.find((skin) => skin.state === "ACTIVE")?.variant;
    if (activeVariant) setVariant(activeVariant.toLowerCase() === "slim" ? "slim" : "classic");
  }, [cosmetics]);

  async function addSkin() {
    setBusy("add"); setError(undefined);
    try {
      const skin = await api.addOfflineSkin(account.id);
      if (skin) { onSkinsChange([skin, ...skins.map((item) => ({ ...item, isActive: false }))]); setSelectedId(skin.id); }
    } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  async function applySkin() {
    if (!selected) return;
    setBusy("skin"); setError(undefined);
    try {
      onCosmeticsChange(await api.applyMinecraftSkin(account.id, selected.id, variant)); setSelectedId(null);
      onSkinsChange(skins.map((item) => ({ ...item, isActive: item.id === selected.id })));
    } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  async function setCape(capeId?: string) {
    setBusy(`cape:${capeId ?? "none"}`); setError(undefined);
    try { onCosmeticsChange(await api.activateMinecraftCape(account.id, capeId)); }
    catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  return <section className="skins-page cosmetics-page">
    <div className="page-heading"><span className="eyebrow">Minecraft-профиль</span><h1>Скины и плащи</h1><p>Управляйте внешностью лицензионного профиля {account.minecraftName}.</p></div>
    {error || loadError ? <div className="cosmetics-alert" role="alert">{error ?? loadError}{loadError ? <button onClick={onRefresh} type="button">Повторить</button> : null}</div> : null}
    <div className="cosmetics-layout">
      <section className="skin-viewer-card cosmetics-preview">
        <div className="preview-badge">{selected ? "Предпросмотр" : "Текущий скин"}</div>
        {previewSkin ? <SkinCanvas skin={previewSkin} cape={selected ? undefined : cosmetics?.capes.find((cape) => cape.state === "ACTIVE")?.url} /> : <div className="skin-empty"><span className="skin-empty-icon">＋</span>Добавьте PNG-скин<br />64×64 или 64×32</div>}
        <div className="preview-meta"><strong>{selected?.name ?? account.minecraftName}</strong></div>
      </section>
      <div className="cosmetics-content">
        <section className="cosmetics-panel">
          <div className="cosmetics-panel-head"><div><span className="section-kicker">Библиотека</span><h2>Мои скины</h2><p>PNG хранятся на этом компьютере и доступны для быстрой смены.</p></div><button className="add-png-button" disabled={Boolean(busy)} onClick={() => void addSkin()} type="button"><span>＋</span>{busy === "add" ? "Добавление…" : "Добавить PNG"}</button></div>
          <div className="saved-skin-grid cosmetics-skin-grid">
            {licensedSkin ? <button aria-label="Текущий скин аккаунта" className={!selected ? "saved-skin active licensed-skin" : "saved-skin licensed-skin"} onClick={() => setSelectedId(null)} title="Текущий скин аккаунта" type="button"><SkinCanvas skin={licensedSkin.url} compact /></button> : null}
            {skins.map((skin) => <button aria-label={`Выбрать скин ${skin.name}`} className={selected?.id === skin.id ? "saved-skin active" : "saved-skin"} key={skin.id} onClick={() => setSelectedId(skin.id)} title={skin.name} type="button"><SkinCanvas skin={skin.dataUrl} compact /></button>)}
            {!skins.length ? <button className="skin-library-empty" onClick={() => void addSkin()} type="button"><span>＋</span><strong>Добавить первый скин</strong><small>PNG · 64×64 или 64×32</small></button> : null}
          </div>
          {selected ? <div className="skin-apply-bar"><div className="variant-switch" aria-label="Модель скина"><button className={variant === "classic" ? "active" : ""} onClick={() => setVariant("classic")} type="button">Классическая</button><button className={variant === "slim" ? "active" : ""} onClick={() => setVariant("slim")} type="button">Тонкая</button></div><button className="primary-cosmetics-action" disabled={Boolean(busy)} onClick={() => void applySkin()} type="button">{busy === "skin" ? "Устанавливаем…" : "Установить на аккаунт"}</button></div> : null}
        </section>
        <section className="cosmetics-panel capes-panel">
          <div className="cosmetics-panel-head"><div><span className="section-kicker">Коллекция аккаунта</span><h2>Плащи</h2><p>Доступны только плащи, полученные этим Minecraft-аккаунтом.</p></div></div>
          {cosmetics ? <div className="cape-grid"><button className={cosmetics.capes.every((cape) => cape.state !== "ACTIVE") ? "cape-card active" : "cape-card"} disabled={Boolean(busy)} onClick={() => void setCape()} type="button"><div className="cape-none">Без плаща</div><strong>Не использовать</strong></button>{cosmetics.capes.map((cape) => <button className={cape.state === "ACTIVE" ? "cape-card active" : "cape-card"} disabled={Boolean(busy)} key={cape.id} onClick={() => void setCape(cape.id)} type="button"><span className="cape-texture"><img alt={`Плащ ${cape.alias}`} src={secureUrl(cape.url)} /></span><strong>{capeName(cape.alias)}</strong><small>{cape.state === "ACTIVE" ? "Надет" : "Надеть"}</small></button>)}{!cosmetics.capes.length ? <div className="capes-empty"><span>◇</span><div><strong>Плащей пока нет</strong><p>Когда плащ появится на аккаунте, он автоматически отобразится здесь.</p></div></div> : null}</div> : <div className="capes-empty"><span>◇</span><div><strong>{loading ? "Загружаем плащи…" : "Данные пока недоступны"}</strong><p>{loading ? "Получаем коллекцию из Minecraft Services." : "Обновите данные профиля повторно."}</p></div></div>}
        </section>
      </div>
    </div>
  </section>;
}

function SkinCanvas({ skin, cape, compact = false }: { skin: string; cape?: string; compact?: boolean }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => { if (!canvas.current) return; const viewer = new SkinViewer({ canvas: canvas.current, width: compact ? 190 : 280, height: compact ? 260 : 340, skin: secureUrl(skin) }); if (cape) void viewer.loadCape(secureUrl(cape)); viewer.autoRotate = false; viewer.zoom = compact ? 1.45 : 0.88; viewer.playerObject.rotation.y = 0.34; if (compact) { viewer.playerWrapper.scale.setScalar(1.08); viewer.playerWrapper.position.y = -3; } viewer.controls.enableRotate = !compact; viewer.controls.enablePan = false; viewer.controls.enableZoom = false; return () => viewer.dispose(); }, [cape, compact, skin]);
  return <canvas className={compact ? "skin-canvas compact" : "skin-canvas"} ref={canvas} />;
}

function secureUrl(url: string) { return url.replace("http://", "https://"); }
function capeName(alias: string) { return alias.toLowerCase().split("_").map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(" "); }
function errorMessage(error: unknown) { if (typeof error === "object" && error && "message" in error && typeof error.message === "string") return error.message; return "Не удалось изменить внешний вид Minecraft-профиля."; }
