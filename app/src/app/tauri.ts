import { invoke } from "@tauri-apps/api/core";

import type { AccountSummary } from "./types";

export interface LauncherApi {
  listAccounts(): Promise<AccountSummary[]>;
  beginMicrosoftLogin(): Promise<AccountSummary>;
  removeAccount(accountId: string): Promise<void>;
  setActiveAccount(accountId: string): Promise<void>;
}

export const launcherApi: LauncherApi = {
  listAccounts: () => invoke<AccountSummary[]>("list_accounts"),
  beginMicrosoftLogin: () => invoke<AccountSummary>("begin_microsoft_login"),
  removeAccount: (accountId) => invoke<void>("remove_account", { accountId }),
  setActiveAccount: (accountId) =>
    invoke<void>("set_active_account", { accountId }),
};
