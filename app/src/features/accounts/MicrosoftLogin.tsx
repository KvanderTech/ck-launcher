import { useState } from "react";

import { launcherApi, type LauncherApi } from "../../app/tauri";
import type { AccountSummary, LauncherErrorDto } from "../../app/types";

interface MicrosoftLoginProps {
  api?: LauncherApi;
  onAuthenticated?: (account: AccountSummary) => void;
}

type LoginState = "idle" | "loading" | "error";

export function MicrosoftLogin({
  api = launcherApi,
  onAuthenticated,
}: MicrosoftLoginProps) {
  const [state, setState] = useState<LoginState>("idle");
  const [errorMessage, setErrorMessage] = useState<string>();

  async function beginLogin() {
    setState("loading");
    setErrorMessage(undefined);
    try {
      const account = await api.beginMicrosoftLogin();
      setState("idle");
      onAuthenticated?.(account);
    } catch (error: unknown) {
      setErrorMessage(errorMessageFrom(error));
      setState("error");
    }
  }

  return (
    <div className="microsoft-login">
      <button
        disabled={state === "loading"}
        onClick={() => void beginLogin()}
        type="button"
      >
        {state === "loading"
          ? "Входим…"
          : state === "error"
            ? "Повторить вход"
            : "Войти через Microsoft"}
      </button>
      {state === "error" ? <p role="alert">{errorMessage}</p> : null}
    </div>
  );
}

function errorMessageFrom(error: unknown): string {
  if (isLauncherError(error)) return error.message;
  return "Не удалось войти через Microsoft.";
}

function isLauncherError(error: unknown): error is LauncherErrorDto {
  if (typeof error !== "object" || error === null) return false;
  const candidate = error as Partial<LauncherErrorDto>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}
