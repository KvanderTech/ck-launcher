# ЦК Лаунчер Windows Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Собрать Windows x64-приложение «ЦК Лаунчер», которое авторизует лицензионный Microsoft-аккаунт, устанавливает официальную Vanilla-версию Minecraft с нужной Java и запускает игру.

**Architecture:** React/TypeScript отображает утверждённый интерфейс и вызывает строго типизированные Tauri-команды. Rust-ядро владеет секретами, SQLite, сетью, файловой системой, установкой Java/Minecraft и дочерним процессом игры; длительные операции отправляют события с идентификатором операции.

**Tech Stack:** Tauri 2, React, TypeScript, Vite, Rust stable MSVC, Tokio, Reqwest, Serde, SQLx/SQLite, keyring, tracing, Vitest, React Testing Library, Cargo tests.

**Spec:** `docs/superpowers/specs/2026-08-23-ck-launcher-windows-core-design.md`

## Global Constraints

- Целевая платформа первого релиза: Windows 10/11 x64.
- Данные приложения: `%AppData%\CKLauncher`; секреты: Windows Credential Manager.
- OAuth: системный браузер, loopback callback `127.0.0.1`, PKCE и проверка `state`.
- React никогда не получает refresh-токены и не формирует команду Java.
- Поддерживаемые runtime: Java 8, 17, 21 и 25.
- Первый релиз показывает стабильные официальные Vanilla-релизы; snapshots и модлоадеры исключены.
- Все загрузки завершаются через `.part`, атомарное переименование и SHA-проверку, когда SHA доступен.
- Запуск Java выполняется без `cmd.exe`; секреты очищаются из журналов и ошибок.
- Выбора темы нет; используется утверждённое тёмное оформление.

## Карта файлов

```text
app/
├─ package.json                         # frontend-команды и зависимости
├─ src/
│  ├─ app/App.tsx                       # маршрутизация экранов и общие состояния
│  ├─ app/tauri.ts                      # единственная TypeScript-граница invoke/listen
│  ├─ app/types.ts                      # DTO, совпадающие с Rust
│  ├─ features/accounts/                # вход и переключатель аккаунтов
│  ├─ features/home/                    # профиль, версия и запуск
│  ├─ features/settings/                # память и Java
│  ├─ components/                       # общие кнопки, панели, прогресс
│  ├─ styles/                           # токены и адаптированный макет
│  └─ test/                             # Vitest setup и mock Tauri
└─ src-tauri/
   ├─ Cargo.toml
   ├─ tauri.conf.json
   ├─ capabilities/default.json         # минимальные разрешения Tauri
   └─ src/
      ├─ lib.rs                         # Builder, состояние и регистрация команд
      ├─ error.rs                       # LauncherError и очистка секретов
      ├─ paths.rs                       # корни и безопасное соединение путей
      ├─ storage/                       # SQLite, миграции, credential store
      ├─ auth/                          # PKCE, loopback и цепочка Microsoft/Xbox/MC
      ├─ metadata/                      # официальные manifests и наследование JSON
      ├─ runtime/                       # выбор, обнаружение и установка Java
      ├─ downloads/                     # план и исполняемая очередь загрузок
      ├─ installer/                     # клиент, libraries, assets, natives
      ├─ profiles/                      # профиль и настройки
      ├─ launcher/                      # аргументы и дочерний процесс
      ├─ commands/                      # узкие Tauri-команды
      └─ tests/fixtures/                 # фиксированные JSON без сетевой зависимости
```

---

### Task 1: Рабочий каркас Tauri и тестовые контуры

**Files:**
- Create: `app/**` официальным React/TypeScript шаблоном Tauri
- Modify: `app/src-tauri/tauri.conf.json`
- Create: `app/src/app/types.ts`
- Create: `app/src/test/setup.ts`
- Test: `app/src/app/types.test.ts`

**Interfaces:**
- Produces: frontend-команды `pnpm test`, `pnpm build`, `pnpm tauri dev`; Rust-команды `cargo test`, `cargo clippy`.
- Produces: `OperationId`, `LauncherStage`, `LauncherErrorDto`, `ProgressEvent`.

- [ ] **Step 1: Создать проект и установить тестовый стек**

Run:

```powershell
pnpm create tauri-app@latest app --template react-ts --manager pnpm --yes
Set-Location app
pnpm add -D vitest jsdom @testing-library/react @testing-library/jest-dom
```

Expected: созданы `app/src` и `app/src-tauri`, `pnpm tauri dev` открывает стандартное окно.

- [ ] **Step 2: Записать падающий тест сериализуемого события**

```ts
// app/src/app/types.test.ts
import { describe, expect, it } from "vitest";
import { progressLabel } from "./types";

describe("progressLabel", () => {
  it("formats byte progress", () => {
    expect(progressLabel({ operationId: "op-1", stage: "downloading", completedBytes: 512, totalBytes: 1024, currentFile: "client.jar" }))
      .toBe("Загрузка client.jar · 50%");
  });
});
```

- [ ] **Step 3: Запустить тест и подтвердить ожидаемое падение**

Run: `pnpm test -- --run src/app/types.test.ts`  
Expected: FAIL — `progressLabel` не экспортируется.

- [ ] **Step 4: Добавить DTO и минимальное форматирование**

```ts
export type OperationId = string;
export type LauncherStage = "idle" | "authenticating" | "resolving-java" | "checking" | "downloading" | "launching" | "running" | "failed";
export interface ProgressEvent { operationId: OperationId; stage: LauncherStage; completedBytes: number; totalBytes: number; currentFile?: string }
export interface LauncherErrorDto { code: string; message: string; details?: string; recoverable: boolean }
export const progressLabel = (p: ProgressEvent) => p.stage === "downloading"
  ? `Загрузка ${p.currentFile ?? "файлов"} · ${p.totalBytes ? Math.floor(p.completedBytes / p.totalBytes * 100) : 0}%`
  : p.stage;
```

- [ ] **Step 5: Настроить Vitest и проверить оба контура**

Run: `pnpm test -- --run; pnpm build; cargo test --manifest-path src-tauri/Cargo.toml`  
Expected: все команды завершаются с кодом 0.

- [ ] **Step 6: Добавить проверку WebView2 при старте Windows-приложения**

Rust startup проверяет доступность WebView2 Runtime до показа основного окна. При отсутствии приложение показывает нативное сообщение с официальной ссылкой установки и завершает запуск; unit test adapter-а проверяет состояния `available` и `missing` без зависимости от реестра тестовой машины.

- [ ] **Step 7: Commit**

```powershell
git add app
git commit -m "build: scaffold Tauri React launcher"
```

---

### Task 2: Ошибки, пути и SQLite

**Files:**
- Create: `app/src-tauri/src/error.rs`
- Create: `app/src-tauri/src/paths.rs`
- Create: `app/src-tauri/src/storage/mod.rs`
- Create: `app/src-tauri/migrations/0001_initial.sql`
- Modify: `app/src-tauri/src/lib.rs`
- Test: inline Rust tests in each module

**Interfaces:**
- Produces: `LauncherError { code, message, details, recoverable }`.
- Produces: `AppPaths::new(base: PathBuf)`, `AppPaths::safe_join(&self, root: &Path, relative: &Path)`.
- Produces: `Storage::connect(database_url: &str)`, `Storage::upsert_profile`, `Storage::active_profile`.

- [ ] **Step 1: Написать падающие тесты безопасности пути и очистки секрета**

```rust
#[test]
fn rejects_parent_escape() {
    let paths = AppPaths::new(PathBuf::from(r"C:\safe"));
    assert!(paths.safe_join(&paths.game, Path::new(r"..\secret.txt")).is_err());
}

#[test]
fn redacts_bearer_tokens() {
    let error = LauncherError::internal("request failed: Bearer abc.def.ghi");
    assert!(!error.details.unwrap().contains("abc.def.ghi"));
}
```

- [ ] **Step 2: Подтвердить падение**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml paths::tests error::tests`  
Expected: FAIL — модули ещё не существуют.

- [ ] **Step 3: Реализовать тип ошибки и канонические корни**

`safe_join` отклоняет абсолютные относительные пути, `ParentDir`, `Prefix` и результат вне выбранного корня. `LauncherError` сериализуется в camelCase и заменяет значения после `Bearer`, `access_token`, `refresh_token` на `[REDACTED]`.

- [ ] **Step 4: Добавить схему SQLite**

```sql
CREATE TABLE accounts (id TEXT PRIMARY KEY, minecraft_name TEXT NOT NULL, minecraft_uuid TEXT NOT NULL, head_url TEXT, is_active INTEGER NOT NULL DEFAULT 0);
CREATE TABLE profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL, version_id TEXT, memory_mb INTEGER NOT NULL DEFAULT 4096, game_dir TEXT NOT NULL, java_override TEXT);
CREATE TABLE settings (key TEXT PRIMARY KEY, value_json TEXT NOT NULL);
CREATE TABLE installations (version_id TEXT PRIMARY KEY, state TEXT NOT NULL, verified_at TEXT);
```

- [ ] **Step 5: Реализовать `Storage` и migration test во временной базе**

Тест создаёт `sqlite::memory:`, применяет миграции, записывает профиль `default`, читает его и проверяет `memory_mb == 4096`.

- [ ] **Step 6: Проверить форматирование, тесты и lint**

Run: `cargo fmt --manifest-path app/src-tauri/Cargo.toml --check; cargo test --manifest-path app/src-tauri/Cargo.toml; cargo clippy --manifest-path app/src-tauri/Cargo.toml -- -D warnings`  
Expected: PASS.

- [ ] **Step 7: Commit**

```powershell
git add app/src-tauri
git commit -m "feat: add secure paths errors and storage"
```

---

### Task 3: Microsoft OAuth и аккаунты

**Files:**
- Create: `app/src-tauri/src/auth/{mod.rs,pkce.rs,loopback.rs,client.rs}`
- Create: `app/src-tauri/src/storage/credentials.rs`
- Create: `app/src-tauri/src/commands/accounts.rs`
- Create: `app/src/features/accounts/{AccountMenu.tsx,MicrosoftLogin.tsx}`
- Test: Rust unit tests and `app/src/features/accounts/AccountMenu.test.tsx`

**Interfaces:**
- Consumes: `Storage`, `LauncherError`.
- Produces: `AuthService::begin_login() -> Result<AuthSession, LauncherError>`.
- Produces: `AuthService::complete_login(session, code) -> Result<AccountSummary, LauncherError>`.
- Produces Tauri commands: `list_accounts`, `begin_microsoft_login`, `remove_account`, `set_active_account`.

- [ ] **Step 1: Написать PKCE/state тесты**

```rust
#[test]
fn pkce_challenge_is_base64url_sha256() {
    let pair = PkcePair::from_verifier("a".repeat(64));
    assert!(!pair.challenge.contains('='));
    assert_eq!(pair.challenge.len(), 43);
}

#[test]
fn callback_rejects_wrong_state() {
    assert!(validate_callback("expected", "wrong", Some("code")).is_err());
}
```

- [ ] **Step 2: Реализовать PKCE и loopback server**

Loopback привязывается только к `127.0.0.1:0`, принимает один callback, сравнивает state постоянным по времени сравнением, отвечает локальной HTML-страницей «Можно вернуться в ЦК Лаунчер» и закрывается по таймауту.

- [ ] **Step 3: Реализовать последовательность токенов за интерфейсом HTTP-клиента**

```rust
#[async_trait]
pub trait MicrosoftApi: Send + Sync {
    async fn exchange_code(&self, code: &str, verifier: &str, redirect_uri: &str) -> Result<OAuthTokens, LauncherError>;
    async fn xbox_live(&self, access_token: &str) -> Result<XboxToken, LauncherError>;
    async fn xsts(&self, xbox: &XboxToken) -> Result<XstsToken, LauncherError>;
    async fn minecraft(&self, xsts: &XstsToken) -> Result<MinecraftAccess, LauncherError>;
    async fn profile(&self, token: &MinecraftAccess) -> Result<AccountSummary, LauncherError>;
}
```

Тест mock-реализации подтверждает порядок вызовов и отдельный код `minecraft_not_owned` при HTTP 404 профиля.

- [ ] **Step 4: Добавить Windows Credential Manager adapter**

Сервис `ck-launcher`, username — стабильный account id. Тесты бизнес-логики используют `InMemoryCredentialStore`; реальный `keyring` adapter покрывается ручной Windows-проверкой.

- [ ] **Step 5: Написать React-тест переключателя аккаунтов**

Mock `list_accounts` возвращает два аккаунта; клик по второму вызывает `set_active_account` с его id и обновляет имя в нижней панели.

- [ ] **Step 6: Реализовать экран входа и меню аккаунтов через `app/tauri.ts`**

Ни один компонент не импортирует `@tauri-apps/api/core` напрямую; все вызовы проходят через типизированные функции `launcherApi`.

- [ ] **Step 7: Запустить Rust и frontend тесты**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml auth; Set-Location app; pnpm test -- --run src/features/accounts`  
Expected: PASS.

- [ ] **Step 8: Commit**

```powershell
git add app
git commit -m "feat: add Microsoft account authentication"
```

---

### Task 4: Официальные версии и профили

**Files:**
- Create: `app/src-tauri/src/metadata/{mod.rs,models.rs,resolver.rs}`
- Create: `app/src-tauri/src/tests/fixtures/version_manifest_v2.json`
- Create: `app/src-tauri/src/tests/fixtures/version_inherited.json`
- Create: `app/src-tauri/src/profiles/mod.rs`
- Create: `app/src-tauri/src/commands/versions.rs`
- Test: Rust fixture tests

**Interfaces:**
- Produces: `MetadataService::stable_releases() -> Result<Vec<GameVersionSummary>, LauncherError>`.
- Produces: `MetadataService::resolved_version(id: &str) -> Result<ResolvedVersion, LauncherError>`.
- Produces commands: `list_game_versions`, `get_profile`, `update_profile`.

- [ ] **Step 1: Зафиксировать минимальные JSON fixtures и написать failing tests**

Тест manifest исключает `snapshot`; тест наследования объединяет `libraries`, заменяет scalar `mainClass` дочерним значением и сохраняет аргументы родителя перед дочерними.

- [ ] **Step 2: Реализовать serde-модели только используемых полей**

Модели включают `id`, `type`, `url`, `sha1`, `downloads`, `assetIndex`, `libraries`, `logging`, `javaVersion`, `arguments`, `minecraftArguments`, `inheritsFrom`.

- [ ] **Step 3: Реализовать кэш с ETag и last-known-good**

HTTP 304 использует кэш; сетевой сбой использует непустой ранее проверенный manifest; отсутствие обоих возвращает `metadata_unavailable`.

- [ ] **Step 4: Реализовать профиль и ограничение памяти**

```rust
pub fn clamp_memory(requested_mb: u32, physical_mb: u64) -> u32 {
    let safe_max = ((physical_mb * 3 / 4).min(32768) as u32 / 512) * 512;
    requested_mb.clamp(512, safe_max.max(512))
}
```

Тесты: 4096 из 16384 остаётся 4096; 20000 из 16384 становится 12288; 128 становится 512.

- [ ] **Step 5: Проверить тесты и commit**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml metadata profiles`  
Expected: PASS.

```powershell
git add app/src-tauri
git commit -m "feat: resolve Minecraft versions and profiles"
```

---

### Task 5: Выбор и установка Java

**Files:**
- Create: `app/src-tauri/src/runtime/{mod.rs,detect.rs,install.rs,archive.rs}`
- Create: `app/src-tauri/src/commands/runtime.rs`
- Create: `app/src/features/settings/{MemorySettings.tsx,JavaSettings.tsx}`
- Test: Rust runtime tests and React settings tests

**Interfaces:**
- Consumes: `ResolvedVersion`, `AppPaths`, `DownloadService`.
- Produces: `RuntimeManager::resolve(requirement: JavaRequirement, override_path: Option<PathBuf>) -> Result<JavaRuntimeStatus, LauncherError>`.
- Produces commands: `runtime_statuses`, `detect_runtime`, `install_runtime`, `choose_runtime_path`.

- [ ] **Step 1: Написать failing test приоритета Java**

Табличный тест отдельно прогоняет Java 8, Java 17, Java 21 и Java 25 и проверяет порядок: управляемая Java нужной major-версии → ручной путь → системный путь; неверная major-версия никогда не выбирается.

- [ ] **Step 2: Реализовать `java -version` probe**

Процесс получает таймаут 5 секунд; parser понимает `1.8`, `17`, `21`, `25`; путь хранится только после успешного probe.

- [ ] **Step 3: Написать и реализовать archive traversal test**

Архив с записью `../../escape.exe` возвращает `unsafe_archive_path`; распаковка идёт во временный каталог и атомарно заменяет runtime только после probe.

- [ ] **Step 4: Реализовать установку runtime через конфиг поставщика**

Список URL и SHA не зашивается в UI. Rust получает versioned manifest поддерживаемого поставщика, выбирает Windows x64 archive, проверяет SHA-256 и устанавливает в `runtime/java-{major}`.

- [ ] **Step 5: Реализовать память и Java в интерфейсе**

Ползунок шагает по 512 МБ, показывает точное значение; четыре Java-карточки отображают `valid`, `missing`, `installing`, `invalid`. Кнопки блокируются на время своей операции.

- [ ] **Step 6: Проверить оба контура и commit**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml runtime; Set-Location app; pnpm test -- --run src/features/settings`  
Expected: PASS.

```powershell
git add app
git commit -m "feat: manage Java runtimes and memory"
```

---

### Task 6: Надёжная очередь загрузок

**Files:**
- Create: `app/src-tauri/src/downloads/{mod.rs,plan.rs,worker.rs,verify.rs}`
- Create: `app/src-tauri/src/downloads/tests.rs`

**Interfaces:**
- Produces: `DownloadSpec { url, destination, expected_size, sha1, sha256 }`.
- Produces: `DownloadService::execute(operation_id, specs, cancel_token, progress_sink) -> Result<(), LauncherError>`.

- [ ] **Step 1: Написать интеграционный тест локального HTTP-сервера**

Сервер отдаёт 1024 фиксированных байта. Тест проверяет создание `.part`, финальное атомарное имя и SHA-1. Второй запуск не делает HTTP-запрос.

- [ ] **Step 2: Написать тест повреждения и повтора**

Первая выдача имеет неверное содержимое, вторая верное; ожидается ровно два запроса и корректный финальный файл.

- [ ] **Step 3: Реализовать verifier и planner**

Корректным считается только обычный файл с ожидаемым размером и хэшем. Отсутствующий хэш не отключает проверку размера.

- [ ] **Step 4: Реализовать worker pool**

Не более шести одновременных запросов; три попытки с экспоненциальной задержкой и jitter; отмена прекращает новые запросы и оставляет `.part`; progress содержит общий объём и текущий файл.

- [ ] **Step 5: Запустить изолированные тесты и commit**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml downloads -- --nocapture`  
Expected: PASS без внешней сети.

```powershell
git add app/src-tauri/src/downloads
git commit -m "feat: add resumable verified downloads"
```

---

### Task 7: Установщик Vanilla Minecraft

**Files:**
- Create: `app/src-tauri/src/installer/{mod.rs,assets.rs,libraries.rs,natives.rs}`
- Create: `app/src-tauri/src/installer/tests.rs`
- Create: `app/src-tauri/src/commands/install.rs`

**Interfaces:**
- Consumes: `ResolvedVersion`, `DownloadService`, `AppPaths`.
- Produces: `Installer::plan(version) -> Result<InstallPlan, LauncherError>`.
- Produces: `Installer::install(operation_id, version_id, cancel_token) -> Result<InstallationSummary, LauncherError>`.
- Produces commands: `install_version`, `cancel_operation`, `installation_status` and event `launcher://progress`.

- [ ] **Step 1: Написать fixture-тест плана установки**

Ожидаемый план содержит client JAR, logging config, asset index, два assets, обычную library и Windows native classifier; Linux/macOS classifiers отсутствуют.

- [ ] **Step 2: Реализовать rules и Maven paths**

Правило без `os` применяется; `allow windows` применяется; `disallow windows` исключает запись. Maven координата `group:name:version` преобразуется в `group/path/name/version/name-version.jar`.

- [ ] **Step 3: Реализовать assets и natives**

Assets идут в `assets/objects/{first_two_hash_chars}/{hash}`. Natives распаковываются через безопасный archive helper; исключения `META-INF/` и указанные `extract.exclude` не записываются.

- [ ] **Step 4: Добавить состояние установки и Tauri progress**

Команда немедленно возвращает `operationId`; работа выполняется в Tokio task; UI фильтрует события по этому id.

- [ ] **Step 5: Запустить installer tests и commit**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml installer`  
Expected: PASS.

```powershell
git add app/src-tauri
git commit -m "feat: install verified Vanilla Minecraft files"
```

---

### Task 8: Формирование команды и жизненный цикл игры

**Files:**
- Create: `app/src-tauri/src/launcher/{mod.rs,arguments.rs,classpath.rs,process.rs}`
- Create: `app/src-tauri/src/launcher/tests.rs`
- Create: `app/src-tauri/src/commands/launch.rs`

**Interfaces:**
- Consumes: `AccountSummary` плюс внутренний access token, `ResolvedVersion`, `LauncherProfile`, `JavaRuntimeStatus`.
- Produces: `LaunchCommand { executable: PathBuf, args: Vec<OsString>, cwd: PathBuf }`.
- Produces: `Launcher::launch(profile_id) -> Result<OperationId, LauncherError>`.
- Produces events: `launcher://game-started`, `launcher://game-exited`, `launcher://error`.

- [ ] **Step 1: Написать тест подстановки аргументов**

Fixture проверяет `${auth_player_name}`, `${auth_uuid}`, `${auth_access_token}`, `${version_name}`, `${game_directory}`, `${assets_root}`, `${assets_index_name}`, `${natives_directory}`, `${classpath}` и legacy `minecraftArguments`.

- [ ] **Step 2: Написать тест Windows classpath и rules**

Classpath разделяется `;`, содержит только разрешённые library JAR и client JAR, не содержит natives JAR.

- [ ] **Step 3: Реализовать чистый builder**

Builder возвращает executable и массив `OsString`; аргументы не объединяются в shell-строку. JVM получает `-Xms512M`, `-Xmx{memory_mb}M`, natives path и logging argument.

- [ ] **Step 4: Реализовать process supervisor**

Один профиль не запускается повторно, stdout/stderr пишутся в очищенный `logs/latest.log`, PID хранится только в памяти, exit code отправляется событием.

- [ ] **Step 5: Проверить отсутствие shell и секретов**

Тест mock spawner получает ровно `LaunchCommand`; тест журнала подтверждает отсутствие переданного access token.

- [ ] **Step 6: Запустить тесты и commit**

Run: `cargo test --manifest-path app/src-tauri/Cargo.toml launcher`  
Expected: PASS.

```powershell
git add app/src-tauri
git commit -m "feat: build and supervise Minecraft process"
```

---

### Task 9: Перенос утверждённого дизайна в React

**Files:**
- Modify: `app/src/app/App.tsx`
- Create: `app/src/features/home/HomePage.tsx`
- Create: `app/src/components/{Sidebar.tsx,WindowControls.tsx,BackgroundCarousel.tsx,ProgressPanel.tsx}`
- Create: `app/src/styles/{tokens.css,launcher.css}`
- Copy: `design/ck-launcher/ck-launcher-assets/*` to `app/src/assets/`
- Test: `app/src/app/App.test.tsx`, feature component tests

**Interfaces:**
- Consumes: все команды `launcherApi` и `ProgressEvent`.
- Produces: рабочие состояния `signed-out`, `ready`, `installing`, `launching`, `running`, `error`.

- [ ] **Step 1: Написать failing happy-path UI test**

Mock возвращает аккаунт Kvander, версии `1.21.8` и `1.20.1`, профиль 4096 МБ. Тест выбирает `1.21.8`, нажимает «Играть», проверяет вызов `launchOrInstall("default")` и отображение progress 50%.

- [ ] **Step 2: Разделить макет на компоненты без `dangerouslySetInnerHTML`**

Сохранить спокойную синюю палитру, округлённые прямоугольники, смену затемнённых скриншотов, SVG-иконки меню, голову активного скина и аккуратные оконные кнопки.

- [ ] **Step 3: Реализовать конечный автомат интерфейса**

«Играть» блокируется в `installing`, `launching`, `running`; повторное событие с чужим operation id игнорируется; recoverable error предлагает «Повторить», остальные — «Открыть журнал».

- [ ] **Step 4: Подключить настройки и аккаунты**

Память сохраняется с debounce 250 мс; Java-карточки отражают Rust DTO; меню аккаунтов не выходит за sidebar и закрывается после выбора.

- [ ] **Step 5: Проверить UI и production build**

Run: `Set-Location app; pnpm test -- --run; pnpm build`  
Expected: PASS и создан `dist`.

- [ ] **Step 6: Commit**

```powershell
git add app/src
git commit -m "feat: implement launcher interface"
```

---

### Task 10: Сквозная команда запуска и восстановление

**Files:**
- Modify: `app/src-tauri/src/commands/launch.rs`
- Modify: `app/src/app/tauri.ts`
- Modify: `app/src/features/home/HomePage.tsx`
- Test: Rust orchestration tests and frontend integration test

**Interfaces:**
- Produces command: `launch_or_install(profile_id: String) -> Result<OperationId, LauncherError>`.

- [ ] **Step 1: Написать orchestration test**

При отсутствующих файлах ожидаемый порядок: refresh account → resolve metadata → resolve Java → install → launch. При полностью установленной версии `install` не вызывается.

- [ ] **Step 2: Реализовать orchestrator с cancellation token**

Каждая стадия публикует progress; первая ошибка завершает operation единственным `launcher://error`; активная операция хранится в registry по profile id.

- [ ] **Step 3: Написать тест отмены и повторной попытки**

Отмена во время загрузки не запускает процесс. Повторная команда использует проверенные файлы и завершает установку.

- [ ] **Step 4: Подключить единственную кнопку «Играть»**

Frontend не решает, нужна ли установка: он вызывает `launch_or_install`, слушает события и показывает этап.

- [ ] **Step 5: Полная автоматическая проверка**

Run:

```powershell
Set-Location app
pnpm test -- --run
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: все команды завершаются с кодом 0.

- [ ] **Step 6: Commit**

```powershell
git add app
git commit -m "feat: complete install and launch workflow"
```

---

### Task 11: Windows hardening, сборка и приёмка

**Files:**
- Modify: `app/src-tauri/tauri.conf.json`
- Modify: `app/src-tauri/capabilities/default.json`
- Create: `app/docs/windows-acceptance.md`
- Create: `app/docs/privacy-and-logs.md`
- Create: `.github/workflows/windows.yml` if GitHub CI is desired for this repository

**Interfaces:**
- Produces: Windows x64 installer artifact и заполненный acceptance report.

- [ ] **Step 1: Сократить Tauri capabilities**

Оставить только необходимые разрешения окна и opener для OAuth URL. Не разрешать frontend произвольный shell, filesystem или HTTP.

- [ ] **Step 2: Добавить Windows CI**

Workflow устанавливает pnpm и Rust stable MSVC, запускает frontend tests/build, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, затем `pnpm tauri build`.

- [ ] **Step 3: Написать чек-лист ручной приёмки**

Документ содержит отдельные строки результата для Windows 10 x64 и Windows 11 x64: первый запуск, отсутствие WebView2, успешный вход, отмена входа, аккаунт без лицензии, Java 8, Java 21, потеря сети, продолжение, повторный запуск, переключение аккаунта, журнал без секретов.

- [ ] **Step 4: Собрать Windows installer**

Run: `Set-Location app; pnpm tauri build`  
Expected: создан Windows x64 bundle в `app/src-tauri/target/release/bundle/`.

- [ ] **Step 5: Выполнить ручную приёмку**

Заполнить дату, версию Windows, версию приложения, фактический результат каждого пункта и ссылку на очищенный лог ошибки при отклонении.

- [ ] **Step 6: Финальная проверка репозитория**

Run: `git status --short`  
Expected: отсутствуют незапланированные generated files, токены, `.part`, runtime и игровые assets.

- [ ] **Step 7: Commit**

```powershell
git add app .github/workflows/windows.yml
git commit -m "build: harden and package Windows launcher"
```
