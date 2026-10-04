import { KvanthIcon } from "../../components/KvanthIcon";
import { useEffect, useMemo, useRef, useState } from "react";
import { SkinViewer } from "skinview3d";

import type { AppApi } from "../../app/tauri";
import type { AccountSummary, MinecraftCosmetics, OfflineSkin } from "../../app/types";
import { createEmotecraftPreviewAnimation } from "./emotecraftPreview";

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
  const [libraryTab, setLibraryTab] = useState<"all" | "favorites">("all");
  const [query, setQuery] = useState("");
  const [editingId, setEditingId] = useState<string>();
  const [draftName, setDraftName] = useState("");
  const selected = selectedId ? skins.find((skin) => skin.id === selectedId) : undefined;
  const licensedSkin = cosmetics?.skins.find((skin) => skin.state === "ACTIVE") ?? cosmetics?.skins[0];
  const previewSkin = selected?.dataUrl ?? licensedSkin?.url;
  const visibleSkins = useMemo(() => skins.filter((skin) => (libraryTab === "all" || skin.isFavorite) && skin.name.toLocaleLowerCase("ru").includes(query.trim().toLocaleLowerCase("ru"))), [libraryTab, query, skins]);

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

  async function deleteSkin(skin: OfflineSkin) {
    setBusy(`delete:${skin.id}`); setError(undefined);
    try {
      await api.deleteOfflineSkin(account.id, skin.id);
      onSkinsChange(skins.filter((item) => item.id !== skin.id));
      if (selected?.id === skin.id) setSelectedId(null);
    } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  async function favoriteSkin(skin: OfflineSkin) {
    setBusy(`favorite:${skin.id}`); setError(undefined);
    try {
      const changed = await api.setOfflineSkinFavorite(account.id, skin.id, !skin.isFavorite);
      onSkinsChange(skins.map((item) => item.id === skin.id ? changed : item));
    } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  async function renameSkin(skin: OfflineSkin) {
    if (!draftName.trim()) return;
    setBusy(`rename:${skin.id}`); setError(undefined);
    try {
      const changed = await api.renameOfflineSkin(account.id, skin.id, draftName);
      onSkinsChange(skins.map((item) => item.id === skin.id ? changed : item));
      setEditingId(undefined);
    } catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  async function setCape(capeId?: string) {
    setBusy(`cape:${capeId ?? "none"}`); setError(undefined);
    try { onCosmeticsChange(await api.activateMinecraftCape(account.id, capeId)); }
    catch (reason) { setError(errorMessage(reason)); } finally { setBusy(undefined); }
  }

  return <section className="skins-page cosmetics-page">
    <div className="page-heading"><h1>Скины и плащи</h1></div>
    {error || loadError ? <div className="cosmetics-alert" role="alert">{error ?? loadError}{loadError ? <button onClick={onRefresh} type="button"><KvanthIcon name="refresh" size={18} /> Повторить</button> : null}</div> : null}
    <div className="cosmetics-layout">
      <section className="skin-viewer-card cosmetics-preview">
        <div className="preview-badge">{selected ? "Предпросмотр" : "Текущий скин"}</div>
        {previewSkin ? <SkinCanvas skin={previewSkin} cape={selected ? undefined : cosmetics?.capes.find((cape) => cape.state === "ACTIVE")?.url} model={selected ? variant : licensedSkin?.variant.toLowerCase() === "slim" ? "slim" : "classic"} /> : <div className="skin-empty"><span className="skin-empty-icon"><KvanthIcon name="add" size={28} /></span>Добавьте PNG-скин<br />64×64 или 64×32</div>}
        <div className="preview-meta"><strong>{account.minecraftName}</strong></div>
      </section>
      <div className="cosmetics-content">
        <section className="cosmetics-panel">
          <div className="cosmetics-panel-head"><div><h2>Мои скины</h2></div><button className="add-png-button" disabled={Boolean(busy)} onClick={() => void addSkin()} type="button"><span><KvanthIcon name="add" size={28} /></span>{busy === "add" ? "Добавление…" : "Добавить PNG"}</button></div>
          <div className="skin-library-toolbar"><button aria-pressed={libraryTab === "favorites"} className={libraryTab === "favorites" ? "skin-favorites-toggle active" : "skin-favorites-toggle"} onClick={() => setLibraryTab((current) => current === "favorites" ? "all" : "favorites")} type="button"><KvanthIcon name="favorite" size={18} /> Избранное <span>{skins.filter((skin) => skin.isFavorite).length}</span></button><label className="skin-search"><span><KvanthIcon name="search" size={18} /></span><input aria-label="Поиск скинов" onChange={(event) => setQuery(event.target.value)} placeholder="Поиск по названию" value={query} /></label></div>
          <div className="saved-skin-grid cosmetics-skin-grid">
            {libraryTab === "all" && !query && licensedSkin ? <button aria-label="Текущий скин аккаунта" className={skins.some((skin) => skin.isActive) ? "saved-skin licensed-skin" : "saved-skin active licensed-skin"} data-sound="skin-select" onClick={() => setSelectedId(null)} title="Текущий скин аккаунта" type="button"><SkinCanvas skin={licensedSkin.url} compact model={licensedSkin.variant.toLowerCase() === "slim" ? "slim" : "classic"} /></button> : null}
            {visibleSkins.map((skin) => <div className="saved-skin-card" key={skin.id}><button aria-label={`Выбрать скин ${skin.name}`} aria-pressed={selected?.id === skin.id} className={skin.isActive ? "saved-skin active" : "saved-skin"} data-sound="skin-select" disabled={Boolean(busy)} onClick={() => setSelectedId(skin.id)} title={skin.name} type="button"><SkinCanvas skin={skin.dataUrl} compact /></button><button aria-label={skin.isFavorite ? "Убрать из избранного" : "Добавить в избранное"} className={skin.isFavorite ? "favorite-skin-button active" : "favorite-skin-button"} disabled={Boolean(busy)} onClick={() => void favoriteSkin(skin)} title={skin.isFavorite ? "Убрать из избранного" : "В избранное"} type="button"><KvanthIcon name="favorite" size={16} /></button><button aria-label="Переименовать скин" className="rename-skin-button" disabled={Boolean(busy)} onClick={() => { setEditingId(skin.id); setDraftName(skin.name); }} title="Переименовать" type="button"><KvanthIcon name="rename" size={16} /></button><button aria-label="Удалить сохранённый скин" className="delete-skin-button" disabled={Boolean(busy)} onClick={() => void deleteSkin(skin)} title="Удалить скин" type="button"><KvanthIcon name="delete" size={16} /></button>{editingId === skin.id ? <form className="skin-name-editor" onSubmit={(event) => { event.preventDefault(); void renameSkin(skin); }}><input aria-label="Новое название скина" autoFocus maxLength={60} onChange={(event) => setDraftName(event.target.value)} value={draftName} /><button disabled={!draftName.trim() || Boolean(busy)} type="submit"><KvanthIcon name="confirm" size={16} /></button><button onClick={() => setEditingId(undefined)} type="button"><KvanthIcon name="close" size={16} /></button></form> : null}</div>)}
            {libraryTab === "all" && !query ? <button aria-label="Добавить скин" className="skin-library-empty" onClick={() => void addSkin()} title="Добавить скин" type="button"><span><KvanthIcon name="add" size={28} /></span></button> : null}
            {!visibleSkins.length && (libraryTab === "favorites" || Boolean(query.trim())) ? <div className="skin-library-no-results"><span><KvanthIcon name={libraryTab === "favorites" ? "favorite" : "search"} size={32} /></span><p>{libraryTab === "favorites" ? "Добавьте скины звёздочкой" : "Скины не найдены"}</p></div> : null}
          </div>
          {selected ? <div className="skin-apply-bar"><div className="variant-switch" aria-label="Модель скина"><button className={variant === "classic" ? "active" : ""} onClick={() => setVariant("classic")} type="button">Классическая</button><button className={variant === "slim" ? "active" : ""} onClick={() => setVariant("slim")} type="button">Тонкая</button></div><button className="primary-cosmetics-action" disabled={Boolean(busy)} onClick={() => void applySkin()} type="button"><KvanthIcon name="confirm" size={18} /> {busy === "skin" ? "Устанавливаем…" : "Установить на аккаунт"}</button></div> : null}
        </section>
        <section className="cosmetics-panel capes-panel">
          <div className="cosmetics-panel-head"><div><h2>Плащи</h2></div></div>
          {cosmetics ? <div className="cape-grid"><button aria-label="Без плаща" aria-pressed={cosmetics.capes.every((cape) => cape.state !== "ACTIVE")} className={cosmetics.capes.every((cape) => cape.state !== "ACTIVE") ? "cape-card active" : "cape-card"} disabled={Boolean(busy)} onClick={() => void setCape()} type="button"><span className="cape-none">×</span></button>{cosmetics.capes.map((cape) => <button aria-label={`Плащ ${capeName(cape.alias)}`} aria-pressed={cape.state === "ACTIVE"} className={cape.state === "ACTIVE" ? "cape-card active" : "cape-card"} disabled={Boolean(busy)} key={cape.id} onClick={() => void setCape(cape.id)} title={capeName(cape.alias)} type="button"><span className="cape-texture"><img alt="" src={secureUrl(cape.url)} /></span></button>)}{!cosmetics.capes.length ? <div className="capes-empty"><span><KvanthIcon name="skins" size={32} /></span><div><strong>Плащей пока нет</strong><p>Когда плащ появится на аккаунте, он автоматически отобразится здесь.</p></div></div> : null}</div> : <div className="capes-empty"><span><KvanthIcon name="skins" size={32} /></span><div><strong>{loading ? "Загружаем плащи…" : "Данные пока недоступны"}</strong><p>{loading ? "Получаем коллекцию из Minecraft Services." : "Обновите данные профиля повторно."}</p></div></div>}
        </section>
      </div>
    </div>
  </section>;
}

function SkinCanvas({ skin, cape, compact = false, model }: { skin: string; cape?: string; compact?: boolean; model?: "classic" | "slim" }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => { if (!canvas.current) return; const viewer = new SkinViewer({ canvas: canvas.current, width: compact ? 190 : 280, height: compact ? 260 : 340, skin: secureUrl(skin), model: model === "classic" ? "default" : model }); if (cape) void viewer.loadCape(secureUrl(cape)); viewer.autoRotate = false; viewer.zoom = compact ? 1.45 : 0.88; viewer.playerWrapper.rotation.y = 0.34; if (compact) { viewer.playerWrapper.scale.setScalar(1.08); viewer.playerWrapper.position.y = -7.5; } else { viewer.animation = createEmotecraftPreviewAnimation(); } viewer.controls.enableRotate = !compact; viewer.controls.enablePan = false; viewer.controls.enableZoom = false; return () => viewer.dispose(); }, [cape, compact, model, skin]);
  return <canvas className={compact ? "skin-canvas compact" : "skin-canvas"} ref={canvas} />;
}

function secureUrl(url: string) { return url.replace("http://", "https://"); }
function capeName(alias: string) { return alias.toLowerCase().split("_").map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(" "); }
function errorMessage(error: unknown) { if (typeof error === "object" && error && "message" in error && typeof error.message === "string") return error.message; return "Не удалось изменить внешний вид Minecraft-профиля."; }
