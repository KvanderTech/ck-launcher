import { KvanthIcon } from "../../components/KvanthIcon";
import { useEffect, useMemo, useRef, useState } from "react";

import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary, MinecraftCosmetics } from "../../app/types";
import { AccountAvatar } from "./AccountAvatar";
import { MicrosoftLogin } from "./MicrosoftLogin";

interface AccountMenuProps {
  accounts: AccountSummary[];
  cosmeticsByAccount?: Record<string, MinecraftCosmetics>;
  api?: LauncherApi;
  closeSignal?: string;
  onActiveAccountChange?: (account: AccountSummary) => void;
  onAuthenticated?: (account: AccountSummary) => void;
  onAccountRemoved?: (accountId: string) => void;
}

export function AccountMenu({
  accounts,
  cosmeticsByAccount = {},
  api = launcherApi,
  closeSignal,
  onActiveAccountChange,
  onAuthenticated,
  onAccountRemoved,
}: AccountMenuProps) {
  const initialActiveId = accounts.find((account) => account.isActive)?.id;
  const [activeId, setActiveId] = useState(initialActiveId);
  const [switchingId, setSwitchingId] = useState<string>();
  const [removing, setRemoving] = useState(false);
  const [open, setOpen] = useState(false);
  const switcherRef = useRef<HTMLElement>(null);
  const activeAccount = useMemo(
    () => accounts.find((account) => account.id === activeId) ?? accounts[0],
    [accounts, activeId],
  );

  useEffect(() => {
    setOpen(false);
  }, [closeSignal]);

  useEffect(() => {
    if (!open) return;
    function closeOutside(event: PointerEvent) {
      if (!switcherRef.current?.contains(event.target as Node)) setOpen(false);
    }
    document.addEventListener("pointerdown", closeOutside);
    return () => document.removeEventListener("pointerdown", closeOutside);
  }, [open]);

  useEffect(() => {
    const nextActiveId = accounts.find((account) => account.isActive)?.id;
    if (nextActiveId) setActiveId(nextActiveId);
  }, [accounts]);

  async function selectAccount(accountId: string) {
    if (accountId === activeId || switchingId) return;
    setSwitchingId(accountId);
    try {
      await api.setActiveAccount(accountId);
      setActiveId(accountId);
      const selected = accounts.find((account) => account.id === accountId);
      if (selected) onActiveAccountChange?.(selected);
    } finally {
      setSwitchingId(undefined);
      setOpen(false);
    }
  }

  async function removeActiveAccount() {
    if (!activeAccount || removing) return;
    setRemoving(true);
    try {
      await api.removeAccount(activeAccount.id);
      onAccountRemoved?.(activeAccount.id);
      setOpen(false);
    } finally {
      setRemoving(false);
    }
  }

  if (!activeAccount) {
    return null;
  }

  function activeSkin(accountId: string) {
    return cosmeticsByAccount[accountId]?.skins.find(skin => skin.state === 'ACTIVE')?.url;
  }

  return (
    <section aria-label="Аккаунты Minecraft" className="account-switcher" ref={switcherRef}>
      {open ? (
        <div aria-label="Аккаунты Minecraft" className="account-menu" role="menu">
          <span className="account-menu-label">Аккаунты Minecraft</span>
          {accounts.map((account) => (
            <button
              aria-checked={account.id === activeId}
              className="account-choice"
              disabled={Boolean(switchingId)}
              key={account.id}
              onClick={() => void selectAccount(account.id)}
              role="menuitemradio"
              type="button"
            >
              <AccountAvatar key={`${account.id}:${activeSkin(account.id)}`} account={account} skinUrl={activeSkin(account.id)} />
              <span><strong>{account.minecraftName}</strong><small>{account.id === activeId ? "Основной профиль" : "Microsoft"}</small></span>
              {account.id === activeId ? <span aria-hidden="true" className="account-check"><KvanthIcon name="confirm" size={16} /></span> : null}
            </button>
          ))}
          <MicrosoftLogin
            api={api}
            buttonRole="menuitem"
            idleLabel="Добавить аккаунт"
            onAuthenticated={(account) => {
              onAuthenticated?.(account);
              setOpen(false);
            }}
          />
          <button className="account-logout" disabled={removing} onClick={() => void removeActiveAccount()} role="menuitem" type="button">
            <KvanthIcon name="logout" size={18} />{removing ? "Выходим…" : "Выйти из аккаунта"}
          </button>
        </div>
      ) : null}
      <button
        aria-expanded={open}
        aria-label={`${activeAccount.minecraftName} Minecraft account`}
        className="active-account-panel"
        data-testid="active-account-panel"
        onClick={() => setOpen((current) => !current)}
        type="button"
      >
        <AccountAvatar key={`${activeAccount.id}:${activeSkin(activeAccount.id)}`} account={activeAccount} skinUrl={activeSkin(activeAccount.id)} />
        <span className="account-copy">
          <strong>{activeAccount.minecraftName}</strong>
          <small>Minecraft account</small>
        </span>
        <KvanthIcon name="dropdown" size={18} />
      </button>
    </section>
  );
}
