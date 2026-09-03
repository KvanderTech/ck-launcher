import { windowApi, type LauncherApi } from "../app/tauri";
import type { AccountSummary, BuildSummary } from "../app/types";
import logo from "../assets/logo.png";
import { AccountMenu } from "../features/accounts/AccountMenu";

export type PageId = "home" | "library" | "content" | "skins" | "settings";

interface SidebarProps {
  activePage: PageId;
  accounts: AccountSummary[];
  accountApi: LauncherApi;
  builds: BuildSummary[];
  onAccountAdded(account: AccountSummary): void;
  onAccountRemoved(accountId: string): void;
  onActiveAccountChange(account: AccountSummary): void;
  onNavigate(page: PageId): void;
  onOpenBuild(buildId: string): void;
}

const navigation: Array<{ id: PageId; label: string; icon: IconName }> = [
  { id: "home", label: "Главная", icon: "home" },
  { id: "library", label: "Библиотека", icon: "library" },
  { id: "content", label: "Каталог", icon: "blocks" },
  { id: "skins", label: "Скины и плащи", icon: "shirt" },
];

export function Sidebar({
  activePage,
  accounts,
  accountApi,
  builds,
  onAccountAdded,
  onAccountRemoved,
  onActiveAccountChange,
  onNavigate,
  onOpenBuild,
}: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand" onMouseDown={(event) => { if (event.button === 0) void windowApi.startDragging(); }}>
        <img alt="Логотип ЦК" src={logo} />
        <span><strong>ЦК Лаунчер</strong><small>Твой мир — твои правила</small></span>
      </div>
      <nav aria-label="Разделы лаунчера" className="sidebar-primary">
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
      <div aria-hidden="true" className="sidebar-divider" />
      <nav aria-label="Установленные сборки" className="sidebar-builds">
        {builds.map((build) => <button aria-label={build.name} aria-pressed={activePage === "library" && build.isActive} className={build.isActive ? "sidebar-build active" : "sidebar-build"} key={build.id} onClick={() => onOpenBuild(build.id)} title={build.name} type="button">{build.iconUrl ? <img alt="" src={build.iconUrl} /> : <span>{build.name.slice(0, 1).toUpperCase()}</span>}</button>)}
      </nav>
      <button aria-label="Добавить сборку" className="sidebar-add-build" onClick={() => onNavigate("content")} title="Добавить сборку" type="button">＋</button>
      <div className="sidebar-spacer" />
      <button aria-label="Настройки" aria-pressed={activePage === "settings"} className={activePage === "settings" ? "sidebar-bottom-button is-active" : "sidebar-bottom-button"} onClick={() => onNavigate("settings")} title="Настройки" type="button"><MenuIcon name="settings" /></button>
      <AccountMenu
        accounts={accounts}
        api={accountApi}
        closeSignal={activePage}
        onActiveAccountChange={onActiveAccountChange}
        onAuthenticated={onAccountAdded}
        onAccountRemoved={onAccountRemoved}
      />
    </aside>
  );
}

type IconName = "home" | "library" | "blocks" | "shirt" | "settings";

function MenuIcon({ name }: { name: IconName }) {
  const paths: Record<IconName, React.ReactNode> = {
    home: <><path d="m3.5 10.7 8.5-7 8.5 7" /><path d="M5.5 9.7v9.8h13V9.7M9.2 19.5v-6h5.6v6" /></>,
    library: <><rect x="3.5" y="4" width="5" height="16" rx="1.7" /><rect x="10.2" y="4" width="4.6" height="16" rx="1.7" /><path d="m17 5 3.3-.8 3.3 14.7-3.4.8L17 5Z" /></>,
    blocks: <><rect x="3" y="3" width="7" height="7" rx="2" /><rect x="14" y="3" width="7" height="7" rx="2" /><rect x="3" y="14" width="7" height="7" rx="2" /><rect x="14" y="14" width="7" height="7" rx="2" /></>,
    shirt: <path d="M8.2 4.1 5 5.6l-3 5.1 4 2v7.7h12v-7.7l4-2-3-5.1-3.2-1.5A4.1 4.1 0 0 1 12 6.6a4.1 4.1 0 0 1-3.8-2.5Z" />,
    settings: <><path d="M3 7h9M18 7h3M3 17h3M12 17h9" /><circle cx="15" cy="7" r="3" /><circle cx="9" cy="17" r="3" /></>,
  };
  return <svg aria-hidden="true" className="menu-icon" viewBox="0 0 24 24">{paths[name]}</svg>;
}
