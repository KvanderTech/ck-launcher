import type { LauncherApi } from "../app/tauri";
import type { AccountSummary } from "../app/types";
import logo from "../assets/logo.png";
import { AccountMenu } from "../features/accounts/AccountMenu";

export type PageId = "home" | "builds" | "mods" | "library" | "skins" | "settings";

interface SidebarProps {
  activePage: PageId;
  accounts: AccountSummary[];
  accountApi: LauncherApi;
  onAccountAdded(account: AccountSummary): void;
  onActiveAccountChange(account: AccountSummary): void;
  onNavigate(page: PageId): void;
}

const navigation: Array<{ id: PageId; label: string; icon: IconName }> = [
  { id: "home", label: "Главная", icon: "home" },
  { id: "builds", label: "Сборки", icon: "blocks" },
  { id: "mods", label: "Моды", icon: "puzzle" },
  { id: "library", label: "Библиотека", icon: "library" },
  { id: "skins", label: "Скины и плащи", icon: "shirt" },
  { id: "settings", label: "Настройки", icon: "settings" },
];

export function Sidebar({
  activePage,
  accounts,
  accountApi,
  onAccountAdded,
  onActiveAccountChange,
  onNavigate,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <img alt="Логотип ЦК" src={logo} />
        <span><strong>ЦК Лаунчер</strong><small>Твой мир — твои правила</small></span>
      </div>
      <nav aria-label="Разделы лаунчера">
        {navigation.map((item) => (
          <button
            aria-label={item.label}
            aria-pressed={activePage === item.id}
            className={activePage === item.id ? "is-active" : ""}
            key={item.id}
            onClick={() => onNavigate(item.id)}
            type="button"
          >
            <MenuIcon name={item.icon} />
            <span>{item.label}</span>
          </button>
        ))}
      </nav>
      <AccountMenu
        accounts={accounts}
        api={accountApi}
        closeSignal={activePage}
        onActiveAccountChange={onActiveAccountChange}
        onAuthenticated={onAccountAdded}
      />
    </aside>
  );
}

type IconName = "home" | "blocks" | "puzzle" | "library" | "shirt" | "settings";

function MenuIcon({ name }: { name: IconName }) {
  const paths: Record<IconName, React.ReactNode> = {
    home: <><path d="m3 10 9-7 9 7" /><path d="M5 9.5V21h14V9.5M9 21v-7h6v7" /></>,
    blocks: <><path d="m12 2 4.5 2.6v5.2L12 12.4 7.5 9.8V4.6L12 2Z" /><path d="m4.5 11.6 4.5 2.6v5.2L4.5 22 0 19.4v-5.2l4.5-2.6Zm15 0 4.5 2.6v5.2L19.5 22 15 19.4v-5.2l4.5-2.6Z" /></>,
    puzzle: <path d="M14.5 4.5a2.5 2.5 0 1 0-5 0H5v5a2.5 2.5 0 1 0 0 5v5h5a2.5 2.5 0 1 0 5 0h4.5V15a2.5 2.5 0 1 0 0-5V4.5h-5Z" />,
    library: <><path d="M4 4v16M9 4v16M14 5l4 14" /><path d="M2 4h4M7 4h4M12.5 5.5l4-1.1M2 20h4M7 20h4M15.5 19.5l4-1.1" /></>,
    shirt: <path d="M8 4.5 5 6l-3 5 4 2v8h12v-8l4-2-3-5-3-1.5A4.2 4.2 0 0 1 12 7a4.2 4.2 0 0 1-4-2.5Z" />,
    settings: <><path d="M4 7h10M18 7h2M4 17h2M10 17h10" /><circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" /></>,
  };
  return <svg aria-hidden="true" className="menu-icon" viewBox="0 0 24 24">{paths[name]}</svg>;
}
