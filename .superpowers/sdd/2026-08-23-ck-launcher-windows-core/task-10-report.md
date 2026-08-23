# Task 10 report — Сквозная команда запуска и восстановление

## Статус

Готово. Финальная команда `launch_or_install(profile_id)` немедленно возвращает единый operation ID, а Rust-orchestrator последовательно выполняет аутентификацию, metadata, Java, проверку/установку игры и безопасный запуск процесса.

## Реализовано

- Добавлен отдельный orchestration-модуль с типизированными этапами, monotonic progress bridge и stage-tagged terminal errors.
- Существующие `AuthService`, `MetadataService`, `RuntimeManager`, `Installer`, `build_launch` и `Launcher` скомпонованы без дублирования их сетевой, path-validation, download, extraction и process-safety логики.
- Один `OperationRegistry` владеет launch/install workflow: блокирует повторный Play и конфликтующую установку, хранит только первое terminal-состояние, ограничивает terminal history и освобождает reservation после failure/cancel/exit.
- Spawn-boundary атомарно отклоняет уже принятую отмену. После boundary отмена идемпотентна и не завершает запущенную игру.
- Все workflow progress/error и process started/error/exited события связаны с operation ID. Вспомогательная ошибка чтения process output помечается non-terminal, чтобы последующий exit оставался единственным terminal transition.
- Installer умеет принять уже разрешённую metadata без повторного запроса и пропускается только после persisted `verified` и повторной проверки фактических size/hash/path/natives. Повреждение файла переводит следующий запуск в repair path; частично проверенные файлы повторно используются verified download queue.
- Действующий Minecraft access token кешируется только в памяти Rust, привязан к активному account ID и используется до expiry с 30-секундным запасом. Refresh failure выдаёт очищенный `account_reauthentication_required`.
- Frontend вызывает только `launch_or_install`, не принимает решения об установке, отображает текущие metadata/install stages и сохраняет существующую фильтрацию stale operation IDs.

## TDD

- Orchestration RED: отсутствующие workflow types/service; GREEN: 9 mock-тестов на точный порядок, verified skip, cancel в metadata/runtime/install, retry/new ID, duplicate/conflict, error stage/no spawn, post-spawn cancel и monotonic progress.
- Registry RED: отменённая операция проходила `mark_spawned`; GREEN: атомарный spawn-boundary возвращает `download_cancelled` и оставляет `Cancelling`.
- Auth RED: второй launch повторно выполнял refresh chain; GREEN: валидный expiry-aware cache исключает повторные network calls.
- Installer RED: отсутствовала проверка реальных файлов; GREEN: локальная end-to-end установка становится verified, а повреждённый client JAR распознаётся как unverified.
- Frontend RED: Task 9 adapter вызывал `launch`, metadata stage не имел корректного progress/cancel UI; GREEN: единственный final invoke и актуальный progress покрыты тестами.

## Полная проверка

- `cargo fmt --manifest-path app/src-tauri/Cargo.toml --all -- --check` — PASS.
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings` — PASS.
- `cargo test --manifest-path app/src-tauri/Cargo.toml --all-targets` — PASS: 147 unit + 2 integration tests.
- `npm test -- --run` — PASS: 8 files, 27 tests.
- `npm run build` — PASS: TypeScript + Vite production build, 51 modules transformed.
- `git diff --check` — PASS (только ожидаемые Windows line-ending warnings).

## Границы

- Отдельной команды принудительного завершения Minecraft нет; отмена после spawn намеренно не убивает процесс.
- Packaging/signing остаются в Task 11.

## Исправления по ревью — Fix1

- Ошибка `mark_spawned` теперь проходит через единый terminal path: отмена на атомарной spawn-boundary переводит workflow в `Cancelled`, освобождает reservation и выдаёт ровно одно launch-stage событие. Синхронная ошибка orchestrated spawn возвращается Launcher без второго process-terminal события; orchestrator всегда выдаёт stage-tagged workflow error, даже если общий registry уже terminal.
- Native extraction атомарно активирует `.ck-native-manifest.json` с относительным путём, размером и SHA-256 каждого файла. Verified-skip повторно хеширует полный inventory и отклоняет отсутствующий, пустой/повреждённый manifest, изменённый файл и любой extra file.
- Frontend DTO разделяет workflow и process errors. `terminal: false` сохраняет running-state, показывает неблокирующее предупреждение без Retry и очищается после exit.
- Startup больше не преобразует Windows `file:///C:/...` в несовместимый SQLx URL. `Storage::connect_file` создаёт отсутствующие parent directories, открывает/создаёт SQLite по проверенному `Path` и выполняет migrations.
- Проверки: Rust — 151 unit + 2 integration tests, strict clippy и fmt PASS; frontend — 8 files / 27 tests и production build PASS. Clean-profile `npm run tauri dev` smoke создал 45,056-byte SQLite, оставался живым с responding window `ЦК Лаунчер`, после проверки процесс остановлен и временный APPDATA удалён.
