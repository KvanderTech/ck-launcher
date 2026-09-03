import { useEffect, useMemo, useRef, useState } from "react";

import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary } from "../../app/types";
import { MicrosoftLogin } from "./MicrosoftLogin";

interface AccountMenuProps {
  accounts: AccountSummary[];
  api?: LauncherApi;
  closeSignal?: string;
  onActiveAccountChange?: (account: AccountSummary) => void;
  onAuthenticated?: (account: AccountSummary) => void;
  onAccountRemoved?: (accountId: string) => void;
}

export function AccountMenu({
  accounts,
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
              <AccountAvatar account={account} />
              <span><strong>{account.minecraftName}</strong><small>{account.id === activeId ? "Основной профиль" : "Microsoft"}</small></span>
              {account.id === activeId ? <span aria-hidden="true" className="account-check">✓</span> : null}
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
            <span aria-hidden="true">↪</span>{removing ? "Выходим…" : "Выйти из аккаунта"}
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
        <AccountAvatar account={activeAccount} />
        <span className="account-copy">
          <strong>{activeAccount.minecraftName}</strong>
          <small>Minecraft account</small>
        </span>
        <svg aria-hidden="true" viewBox="0 0 16 16"><path d="m4 6 4 4 4-4" /></svg>
      </button>
    </section>
  );
}

function AccountAvatar({ account }: { account: AccountSummary }) {
  return account.headUrl ? (
    <img alt="" height={40} src={account.headUrl} width={40} />
  ) : (
    <span aria-hidden="true" className="avatar-fallback">
      {account.minecraftName.slice(0, 1).toUpperCase()}
    </span>
  );
}
