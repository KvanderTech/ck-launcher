# ЦК Лаунчер

Лаунчер для комфортной игры в Minecraft: Java Edition на Windows.

[Telegram](https://t.me/comfortcentr) · [Discord](https://discord.gg/2CkZsVN8nm)

> Независимый проект. Не является официальным продуктом Minecraft и не связан с Mojang или Microsoft.

## Возможности первой beta

- вход через Microsoft с проверкой лицензии Minecraft;
- отдельная библиотека версий и сборок;
- поиск модпаков, модов, ресурспаков и шейдеров через Modrinth;
- фильтрация по версии Minecraft, загрузчику, категории и среде;
- просмотр описания проекта и выбор конкретного релиза;
- установка сборок из Modrinth и локальных файлов `.mrpack`;
- поддержка Vanilla, Fabric и Quilt при создании и запуске сборок;
- управление установленным контентом, файлами, мирами и журналами;
- установка подходящей Java и настройка оперативной памяти;
- управление скинами и плащами лицензионного аккаунта;
- единый индикатор установки, который сохраняется при переходе между вкладками.

## Безопасность

- пароль вводится только на официальной странице Microsoft;
- авторизация использует OAuth 2.0 Authorization Code с PKCE;
- refresh-токены хранятся локально в Windows Credential Manager;
- токены не передаются веб-интерфейсу лаунчера;
- чувствительные данные удаляются из диагностических журналов;
- лаунчер не обходит лицензию, безопасность аккаунта и ограничения Minecraft.

Microsoft Application (Client) ID: `69a61395-3c9e-485e-8662-dcb1bfa73472`

## Технологии

- Tauri 2 и Rust;
- React 19 и TypeScript;
- SQLite;
- Windows Credential Manager.

## Сборка проекта

Требуются Windows 10/11 x64, Node.js, npm, Rust MSVC и WebView2 Runtime.

```powershell
cd app
npm install
npm run test
npm run tauri -- build
```

Дополнительные сведения: [`app/docs/privacy-and-logs.md`](app/docs/privacy-and-logs.md) и [`app/docs/windows-acceptance.md`](app/docs/windows-acceptance.md).

## Правовая информация

Minecraft является товарным знаком Microsoft Corporation. Проект разрабатывается независимо и не одобрен Mojang или Microsoft. Пользователь обязан соблюдать Minecraft EULA и Minecraft Usage Guidelines.
