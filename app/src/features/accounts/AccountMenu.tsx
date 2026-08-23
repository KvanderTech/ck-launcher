import { useMemo, useState } from "react";

import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary } from "../../app/types";

interface AccountMenuProps {
  accounts: AccountSummary[];
  api?: LauncherApi;
}

export function AccountMenu({ accounts, api = launcherApi }: AccountMenuProps) {
  const initialActiveId = accounts.find((account) => account.isActive)?.id;
  const [activeId, setActiveId] = useState(initialActiveId);
  const [switchingId, setSwitchingId] = useState<string>();
  const activeAccount = useMemo(
    () => accounts.find((account) => account.id === activeId) ?? accounts[0],
    [accounts, activeId],
  );

  async function selectAccount(accountId: string) {
    if (accountId === activeId || switchingId) return;
    setSwitchingId(accountId);
    try {
      await api.setActiveAccount(accountId);
      setActiveId(accountId);
    } finally {
      setSwitchingId(undefined);
    }
  }

  if (!activeAccount) {
    return <p>Аккаунты ещё не добавлены.</p>;
  }

  return (
    <section aria-label="Аккаунты Minecraft">
      <ul className="account-menu">
        {accounts.map((account) => (
          <li key={account.id}>
            <button
              aria-pressed={account.id === activeId}
              disabled={Boolean(switchingId)}
              onClick={() => void selectAccount(account.id)}
              type="button"
            >
              {account.minecraftName}
            </button>
          </li>
        ))}
      </ul>
      <div className="active-account-panel" data-testid="active-account-panel">
        {activeAccount.headUrl ? (
          <img alt="" height={40} src={activeAccount.headUrl} width={40} />
        ) : null}
        <span>{activeAccount.minecraftName}</span>
      </div>
    </section>
  );
}
