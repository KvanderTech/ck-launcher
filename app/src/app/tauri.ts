import { invoke } from "@tauri-apps/api/core";

import type { AccountSummary, JavaMajor, JavaRuntimeStatus } from "./types";

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

export interface RuntimeApi {
  runtimeStatuses(): Promise<JavaRuntimeStatus[]>;
  detectRuntime(requirement: JavaMajor): Promise<JavaRuntimeStatus>;
  installRuntime(requirement: JavaMajor): Promise<JavaRuntimeStatus>;
  chooseRuntimePath(requirement: JavaMajor): Promise<JavaRuntimeStatus | null>;
}

export const runtimeApi: RuntimeApi = {
  runtimeStatuses: () => invoke<JavaRuntimeStatus[]>("runtime_statuses"),
  detectRuntime: (requirement) => invoke<JavaRuntimeStatus>("detect_runtime", { requirement }),
  installRuntime: (requirement) => invoke<JavaRuntimeStatus>("install_runtime", { requirement }),
  chooseRuntimePath: (requirement) => invoke<JavaRuntimeStatus | null>("choose_runtime_path", { requirement }),
};
