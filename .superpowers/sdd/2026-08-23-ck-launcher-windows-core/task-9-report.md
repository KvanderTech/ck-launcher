# Task 9 report — Перенос утверждённого дизайна в React

## Статус

Готово. Утверждённый тёмный синий интерфейс перенесён в React с повторным использованием `logo.png` и трёх фоновых кадров из read-only прототипа.

## Реализовано

- Приложение разделено на `App`, `HomePage`, `Sidebar`, `WindowControls`, `BackgroundCarousel` и `ProgressPanel`.
- Добавлены экраны «Главная» и функциональные «Настройки», а также явно помеченные post-MVP состояния для сборок, модов, библиотеки, скинов и плащей.
- Подключены существующие аккаунты/Microsoft login, стабильные версии, профиль, память, Java DTO/actions, запуск и типизированные события только через `app/tauri.ts`.
- `launchOrInstall(profileId)` оставлен как узкий UI-адаптер над текущей Rust-командой `launch`; Task 10 сможет заменить только его реализацию на финальный orchestrator.
- Реализованы состояния ready/installing/launching/running/recoverable-error/fatal-error, блокировка повторного запуска, отмена загрузки, retry и безопасное fatal-состояние без вывода технических details.
- События принимаются только для текущего operation ID. Легитимное `game-started`, которое Rust отправляет до возврата ID из `launch`, временно буферизуется и применяется только после совпадения ID; посторонние события отбрасываются.
- Память использует backend min/max/step и сохраняет профиль с debounce 250 мс. Java-карточки показывают реальные состояния/источник/версию без ненужного полного пути.
- Account menu закрывается после перехода и переключения. На узком окне sidebar временно расширяется под меню, поэтому popup не перекрывает main pane.
- Добавлены семантические labels/pressed/checked/expanded/live/progressbar, видимый keyboard focus и reduced-motion режим без смены/анимации фона.
- Нет `dangerouslySetInnerHTML`, base64 assets, прямых invoke/imports вне `app/tauri.ts`, произвольных shell/fs/http или theme selector.

## TDD и проверка

- RED зафиксирован для happy path/state/navigation/memory/boundary тестов против исходного placeholder UI.
- Отдельный RED/GREEN зафиксирован для реального порядка `game-started` до возврата operation ID.
- `npm test -- --run --reporter=verbose` — PASS: 6 files, 11 tests.
- `npm run build` — PASS: TypeScript + Vite production build, 51 modules transformed.
- Visual QA в локальном браузере:
  - 1280×720: главный экран соответствует утверждённой композиции; sidebar/account и window controls не пересекаются;
  - 800×720: body остаётся 800×720 без overflow, account popup полностью находится внутри sidebar;
  - настройки: `page-scroll` имеет `overflow: auto`, 648 px client height / 1013 px scroll height, sidebar и controls фиксированы.
- Rust проверки не запускались: Rust-файлы, DTO команд и backend interface не менялись.

## Оставшиеся границы

- Финальная install→launch orchestration относится к Task 10; сейчас `launchOrInstall` вызывает существующую команду `launch`.
- Backend-команды безопасного открытия очищенного журнала пока нет, поэтому fatal-кнопка намеренно disabled с пояснением.

## Исправления по ревью — round 1

- Main window переведено в undecorated-режим; добавлены точечные Tauri window permissions. Drag-region ограничена безопасной частью topbar и не захватывает кнопки. Minimize/maximize/close вызываются через централизованный `WindowApi` и покрыты mock-тестом.
- Индикатор Java теперь запрашивает major для выбранной версии через узкую backend-команду. UI и launch preparation используют один `requirement_for_version`, а устаревшие async-ответы игнорируются.
- `game-exited` очищает вспомогательную recoverable-ошибку, поэтому ready-состояние не соседствует с retry. `cancelling` проведён до `ProgressPanel` с disabled-состоянием «Отменяем…».
- Память сохраняется узкой командой `update_profile_memory`: backend меняет только memory в самом свежем профиле, UI сливает из ответа только memory. Отказ показывает безопасную ошибку и возвращает slider к последнему сохранённому значению; тест с in-flight сохранением проверяет, что версия не откатывается.
- «Добавить аккаунт» в menu переиспользует общий `MicrosoftLogin`: loading, безопасная error и retry остаются в popup, rejected Promise обработан.
- Проверки: `npm test -- --run` — PASS, 7 files / 18 tests; `npm run build` — PASS; `cargo test` — PASS, 136 tests total (134 unit + 2 integration); `cargo fmt --check` — PASS.

## Исправления по ревью — round 2

- Debounce сохранения памяти перенесён в стабильный App-level owner. Переход со страницы настроек больше не уничтожает timer; регрессионный тест меняет slider, сразу переходит на главную и подтверждает запись после 250 мс.
- `MemorySettings` теперь только загружает backend limits и отображает App-owned memory/save state. App оптимистично сливает только memory, игнорирует stale responses, а при отказе возвращает последнее подтверждённое значение и показывает safe error.
- Backend `update_memory` больше не выполняет read/whole-profile upsert. `ProfileStore::update_active_profile_memory` использует один атомарный SQLite `UPDATE profiles SET memory_mb = ? ... RETURNING`, поэтому не может откатить параллельно сохранённую версию. Отсутствующий профиль возвращает stable `profile_not_found`, а не создаёт default-профиль.
- Детерминированная Rust-регрессия запускает отложенный memory update, между его стартом и завершением сохраняет новую game version и проверяет итог: новая версия + новая память.
- Проверки: `npm test -- --run` — PASS, 7 files / 20 tests; `npm run build` — PASS; `cargo test -q` — PASS, 138 tests total (136 unit + 2 integration); `cargo fmt --check` — PASS; `cargo clippy --all-targets -- -D warnings` — PASS.

## Исправления по ревью — round 3

- App-level memory persistence заменена на сериализованную coalescing-очередь: одновременно в backend находится не более одного `updateMemory`, а очередь хранит только самое свежее desired-значение.
- Каждый успешный ответ всегда обновляет confirmed memory, даже если пользователь уже выбрал следующее. После завершения текущего запроса сразу отправляется последнее queued-значение; промежуточные выборы не сохраняются.
- Если падает самый свежий запрос, slider возвращается к последнему подтверждённому backend-ответу и показывает safe error. Постановка нового desired в очередь не сбрасывает честный статус «Сохраняем…».
- Регрессии подтверждают: A success → B failure возвращает UI к A; второй запрос не стартует до завершения первого; A/B/C вызывает backend только с A и C и завершается на C даже после перехода со страницы настроек.
- Lifecycle-флаг очереди повторно активируется при StrictMode effect replay; отдельная регрессия воспроизводит обёртку реального `main.tsx` и подтверждает, что сохранение после replay не теряется.
- Проверки: `npm test -- --run` — PASS, 7 files / 23 tests; `npm run build` — PASS. Rust/backend в round 3 не изменялся; полные Rust test/fmt/clippy остаются зелёными по итогам round 2.
