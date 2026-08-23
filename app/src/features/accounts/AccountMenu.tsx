import { useEffect, useMemo, useState } from "react";

import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary } from "../../app/types";
import { MicrosoftLogin } from "./MicrosoftLogin";

interface AccountMenuProps {
  accounts: AccountSummary[];
  api?: LauncherApi;
  closeSignal?: string;
  onActiveAccountChange?: (account: AccountSummary) => void;
  onAuthenticated?: (account: AccountSummary) => void;
}

export function AccountMenu({
  accounts,
  api = launcherApi,
  closeSignal,
  onActiveAccountChange,
  onAuthenticated,
}: AccountMenuProps) {
  const initialActiveId = accounts.find((account) => account.isActive)?.id;
  const [activeId, setActiveId] = useState(initialActiveId);
  const [switchingId, setSwitchingId] = useState<string>();
  const [open, setOpen] = useState(false);
  const activeAccount = useMemo(
    () => accounts.find((account) => account.id === activeId) ?? accounts[0],
    [accounts, activeId],
  );

  useEffect(() => {
    setOpen(false);
  }, [closeSignal]);

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

  if (!activeAccount) {
    return <p className="account-empty">Аккаунты ещё не добавлены.</p>;
  }

  return (
    <section aria-label="Аккаунты Minecraft" className="account-switcher">
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
