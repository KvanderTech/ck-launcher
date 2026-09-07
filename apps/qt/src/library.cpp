#include "widgets.h"
#include "window.h"
QWidget *LauncherWindow::libraryPage() {
    auto *page = new QWidget;
    auto *layout = pageLayout(page, tr("Твой Minecraft. Твои сборки."),
                              tr("Разные версии игры, моды и миры — в отдельных сборках. Всё "
                                 "нужное для следующего приключения."));
    auto *actions = new QHBoxLayout;
    layout->addLayout(actions);
    button(tr("＋ Создать сборку"), actions, [this] { createBuild(); }, this, true);
    button(tr("Импорт .mrpack"), actions, [this] { importPack(); }, this);
    actions->addStretch();
    button(tr("Обновить"), actions, [this] { refreshLibrary(); }, this);
    libraryHint = new QLabel(tr("Загружаем библиотеку…"));
    libraryHint->setWordWrap(true);
    layout->addWidget(libraryHint);
    buildsTable = table({tr("Сборка"), tr("Minecraft"), tr("Загрузчик")}, layout);
    buildsTable->setObjectName(s("buildsTable"));
    connect(buildsTable, &QTableWidget::itemSelectionChanged, this, [this] {
        int row = buildsTable->currentRow();
        if (row >= 0 && row < builds.size()) {
            selectedBuild = value(builds[row].toObject(), "id");
            refreshContent();
        }
    });
    auto *buildActions = new QHBoxLayout;
    layout->addLayout(buildActions);
    play = button(tr("▶ Играть"), buildActions, [this] { launch(); }, this, true);
    play->setObjectName(s("playButton"));
    play->setEnabled(false);
    stop = button(
        tr("Остановить"), buildActions,
        [this] {
            if (!operationId.isEmpty())
                call(s("stop_game"), {{s("operationId"), operationId}});
        },
        this);
    stop->setEnabled(false);
    button(
        tr("Переименовать"), buildActions,
        [this] {
            if (selectedBuild.isEmpty())
                return;
            bool ok;
            auto name =
                QInputDialog::getText(this, tr("Название сборки"), tr("Новое название"),
                                      QLineEdit::Normal, value(currentBuild(), "name"), &ok);
            if (ok)
                call(
                    s("rename_build"), {{s("buildId"), selectedBuild}, {s("name"), name}},
                    [this](const QJsonValue &) { refreshLibrary(); }, true);
        },
        this);
    button(
        tr("Восстановить"), buildActions,
        [this] {
            if (!selectedBuild.isEmpty())
                call(
                    s("repair_build"), {{s("buildId"), selectedBuild}},
                    [this](const QJsonValue &) { refreshLibrary(); }, true);
        },
        this);
    button(tr("Файлы"), buildActions, [this] { browseFiles(); }, this);
    button(
        tr("Удалить"), buildActions,
        [this] {
            if (selectedBuild.isEmpty())
                return;
            if (QMessageBox::question(
                    this, tr("Удалить сборку?"),
                    tr("«%1» будет перемещена в папку trash лаунчера вместе с её мирами.")
                        .arg(value(currentBuild(), "name")),
                    QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes)
                call(
                    s("delete_build"), {{s("buildId"), selectedBuild}},
                    [this](const QJsonValue &) {
                        selectedBuild.clear();
                        refreshLibrary();
                    },
                    true);
        },
        this);
    auto *title = new QLabel(tr("Состав сборки"));
    title->setProperty("sectionTitle", true);
    layout->addWidget(title);
    installedTable = table({tr("Название"), tr("Версия"), tr("Состояние")}, layout);
    auto *contentActions = new QHBoxLayout;
    layout->addLayout(contentActions);
    button(tr("Открыть каталог"), contentActions, [this] { navigation->setCurrentRow(1); }, this);
    button(
        tr("Добавить файл"), contentActions,
        [this] {
            if (selectedBuild.isEmpty())
                return;
            bool ok;
            auto kind = QInputDialog::getItem(
                this, tr("Тип содержимого"), tr("Что добавить?"),
                {tr("Мод .jar"), tr("Ресурспак .zip"), tr("Шейдер .zip")}, 0, false, &ok);
            if (!ok)
                return;
            const auto type = kind.startsWith(tr("Мод"))      ? s("mod")
                              : kind.startsWith(tr("Ресурс")) ? s("resourcepack")
                                                              : s("shader");
            if (type == s("mod") &&
                QMessageBox::question(
                    this, tr("Доверие к моду"),
                    tr("Мод может выполнять код на компьютере. Вы доверяете его автору?"),
                    QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes)
                return;
            call(
                s("import_local_content"),
                {{s("buildId"), selectedBuild}, {s("projectType"), type}},
                [this](const QJsonValue &) { refreshContent(); }, true);
        },
        this);
    button(
        tr("Включить / отключить"), contentActions,
        [this] {
            int r = installedTable->currentRow();
            if (r < 0 || r >= installed.size())
                return;
            auto i = installed[r].toObject();
            call(
                s("set_installed_content_enabled"),
                {{s("buildId"), selectedBuild},
                 {s("projectId"), value(i, "projectId")},
                 {s("enabled"), !i.value(s("enabled")).toBool()}},
                [this](const QJsonValue &) { refreshContent(); }, true);
        },
        this);
    button(
        tr("Удалить файл"), contentActions,
        [this] {
            int r = installedTable->currentRow();
            if (r < 0 || r >= installed.size())
                return;
            auto i = installed[r].toObject();
            if (QMessageBox::question(
                    this, tr("Удаление содержимого"), tr("Удалить «%1»?").arg(value(i, "title")),
                    QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes)
                call(
                    s("remove_installed_content"),
                    {{s("buildId"), selectedBuild}, {s("projectId"), value(i, "projectId")}},
                    [this](const QJsonValue &) { refreshContent(); }, true);
        },
        this);
    return page;
}
QJsonObject LauncherWindow::currentBuild() const {
    for (const auto &b : builds)
        if (value(b.toObject(), "id") == selectedBuild)
            return b.toObject();
    return {};
}
void LauncherWindow::refreshLibrary() {
    call(s("list_builds"), {}, [this](const QJsonValue &v) {
        builds = v.toArray();
        QSignalBlocker block(buildsTable);
        buildsTable->setRowCount(builds.size());
        int selected = 0;
        for (int r = 0; r < builds.size(); ++r) {
            auto b = builds[r].toObject();
            cells(buildsTable, r, {value(b, "name"), value(b, "gameVersion"), value(b, "loader")});
            if (value(b, "id") == selectedBuild ||
                (selectedBuild.isEmpty() && b.value(s("isActive")).toBool()))
                selected = r;
        }
        libraryHint->setText(
            builds.isEmpty()
                ? tr("Пока нет сборок. Создай первую или импортируй доверенный модпак.")
                : tr("Сборок: %1 · Выбери сборку и нажми «Играть».").arg(builds.size()));
        if (!builds.isEmpty()) {
            buildsTable->selectRow(selected);
            selectedBuild = value(builds[selected].toObject(), "id");
        } else
            selectedBuild.clear();
        refreshContent();
    });
}
void LauncherWindow::refreshContent() {
    if (selectedBuild.isEmpty()) {
        installed = {};
        installedTable->setRowCount(0);
        return;
    }
    const auto id = selectedBuild;
    call(s("list_installed_content"), {{s("buildId"), id}}, [this, id](const QJsonValue &v) {
        if (id != selectedBuild)
            return;
        installed = v.toArray();
        installedTable->setRowCount(installed.size());
        for (int r = 0; r < installed.size(); ++r) {
            auto i = installed[r].toObject();
            cells(installedTable, r,
                  {value(i, "title"), value(i, "versionId"),
                   i.value(s("enabled")).toBool() ? tr("Включён") : tr("Отключён")});
        }
    });
}
void LauncherWindow::createBuild() {
    QDialog dialog(this);
    dialog.setWindowTitle(tr("Новая сборка"));
    auto *layout = new QVBoxLayout(&dialog);
    auto *form = new QFormLayout;
    layout->addLayout(form);
    auto *name = new QLineEdit;
    name->setMaxLength(48);
    name->setPlaceholderText(tr("Например, Мир с друзьями"));
    form->addRow(tr("Название"), name);
    auto *version = new QComboBox;
    version->setEditable(true);
    for (const auto &item : versions)
        version->addItem(value(item.toObject(), "id"));
    if (version->count() == 0)
        version->addItem(s("1.20.1"));
    form->addRow(tr("Minecraft"), version);
    auto *loader = new QComboBox;
    loader->addItems({s("vanilla"), s("fabric"), s("quilt")});
    form->addRow(tr("Загрузчик"), loader);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Ok | QDialogButtonBox::Cancel);
    layout->addWidget(buttons);
    connect(buttons, &QDialogButtonBox::accepted, &dialog, &QDialog::accept);
    connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    if (dialog.exec() == QDialog::Accepted)
        call(
            s("create_build"),
            {{s("name"), name->text()},
             {s("gameVersion"), version->currentText()},
             {s("loader"), loader->currentText()}},
            [this](const QJsonValue &v) {
                selectedBuild = value(v.toObject(), "id");
                refreshLibrary();
            },
            true);
}
void LauncherWindow::launch() {
    if (selectedBuild.isEmpty()) {
        message(tr("Сначала создайте или выберите сборку."), true);
        return;
    }
    call(
        s("select_build"), {{s("buildId"), selectedBuild}},
        [this](const QJsonValue &) {
            call(
                s("launch_or_install"), {{s("profileId"), s("default")}},
                [this](const QJsonValue &v) {
                    operationId = v.toString();
                    play->setEnabled(false);
                    message(tr("Подготавливаем Minecraft. Первое скачивание может занять несколько "
                               "минут."));
                },
                true);
        },
        true);
}
void LauncherWindow::importPack(const QString &path) {
    QJsonObject params;
    if (!path.isEmpty())
        params.insert(s("sourcePath"), path);
    call(s("preview_mrpack"), params, [this](const QJsonValue &v) {
        if (v.isNull())
            return;
        auto p = v.toObject();
        QMessageBox review(
            QMessageBox::Warning, tr("Проверка сборки"),
            tr("%1\nMinecraft %2 · %3\nФайлов для загрузки: %4 · %5 МБ\nВстроенных файлов: "
               "%6\n\n%7")
                .arg(value(p, "name"), value(p, "minecraft"), value(p, "loader"))
                .arg(p.value(s("downloadFiles")).toInt())
                .arg(p.value(s("totalDownloadBytes")).toDouble() / 1048576.0, 0, 'f', 1)
                .arg(p.value(s("overrides")).toInt())
                .arg(value(p, "warning")),
            QMessageBox::Yes | QMessageBox::Cancel, this);
        review.setTextFormat(Qt::PlainText);
        review.setDefaultButton(QMessageBox::Cancel);
        QStringList hosts;
        for (const auto &h : p.value(s("hosts")).toArray())
            hosts << h.toString();
        review.setDetailedText(
            tr("Источники: %1\nSHA-256 архива: %2").arg(hosts.join(s(", ")), value(p, "sha256")));
        review.button(QMessageBox::Yes)->setText(tr("Доверяю автору, установить"));
        if (review.exec() == QMessageBox::Yes)
            call(
                s("confirm_mrpack"), {{s("sha256"), value(p, "sha256")}},
                [this](const QJsonValue &) {
                    refreshLibrary();
                    navigation->setCurrentRow(0);
                },
                true);
    });
}
