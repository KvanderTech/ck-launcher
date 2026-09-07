#include "widgets.h"
#include "window.h"
#include <QPointer>
QWidget *LauncherWindow::settingsPage() {
    auto *page = new QWidget;
    auto *layout = pageLayout(page, tr("Всё под контролем"),
                              tr("Лаунчер подбирает Java по требованиям версии игры. Путь и память "
                                 "можно настроить вручную."));
    auto *memoryRow = new QHBoxLayout;
    layout->addLayout(memoryRow);
    memoryRow->addWidget(new QLabel(tr("Память для игры")));
    memory = new QSpinBox;
    memory->setRange(1024, 65536);
    memory->setSingleStep(512);
    memory->setSuffix(tr(" МБ"));
    memoryRow->addWidget(memory);
    button(
        tr("Сохранить"), memoryRow,
        [this] {
            call(
                s("update_profile_memory"), {{s("memoryMb"), memory->value()}},
                [this](const QJsonValue &v) {
                    profile = v.toObject();
                    memory->setValue(profile.value(s("memoryMb")).toInt());
                },
                true);
        },
        this);
    memoryRow->addStretch();
    javaTable = table({tr("Java"), tr("Состояние"), tr("Путь")}, layout);
    auto *javaActions = new QHBoxLayout;
    layout->addLayout(javaActions);
    for (const auto &pair :
         QList<QPair<QString, QString>>{{tr("Найти"), s("detect_runtime")},
                                        {tr("Установить"), s("install_runtime")},
                                        {tr("Выбрать java.exe"), s("choose_runtime_path")}})
        button(
            pair.first, javaActions,
            [this, method = pair.second] {
                int row = javaTable->currentRow();
                if (row < 0)
                    return;
                const int major = javaTable->item(row, 0)->data(Qt::UserRole).toInt();
                call(
                    method, {{s("requirement"), major}},
                    [this](const QJsonValue &) { refreshRuntimes(); }, true);
            },
            this);
    button(
        tr("Обновить версии"), javaActions,
        [this] {
            loadVersions();
            refreshRuntimes();
        },
        this);
    auto *options = new QHBoxLayout;
    layout->addLayout(options);
    button(
        tr("Папка игры…"), options,
        [this] {
            call(
                s("choose_game_directory"), {},
                [this](const QJsonValue &v) {
                    if (v.isObject())
                        profile = v.toObject();
                },
                true);
        },
        this);
    button(
        tr("Отменить загрузку"), options,
        [this] {
            core->request(s("cancel_content_operation"));
            if (!operationId.isEmpty())
                core->request(s("cancel_operation"), {{s("operationId"), operationId}});
            message(tr("Отмена запрошена…"));
        },
        this);
    auto *updateActions = new QHBoxLayout;
    layout->addLayout(updateActions);
    button(
        tr("Проверить обновление"), updateActions,
        [this] {
            call(s("check_update"), {}, [this](const QJsonValue &v) {
                const auto u = v.toObject();
                if (!u.value(s("available")).toBool()) {
                    message(tr("Установлена актуальная версия этого канала."));
                    return;
                }
                QMessageBox info(this);
                info.setWindowTitle(tr("Доступно обновление"));
                info.setTextFormat(Qt::PlainText);
                info.setText(tr("Версия %1\n%2\n\nОткрыть страницу выпуска?")
                                 .arg(value(u, "version"), value(u, "notes")));
                info.setStandardButtons(QMessageBox::Yes | QMessageBox::No);
                if (info.exec() == QMessageBox::Yes)
                    call(s("open_release_page"), {{s("url"), value(u, "url")}});
            });
        },
        this);
    button(
        tr("Страница проекта"), updateActions,
        [this] {
            call(s("open_external_url"),
                 {{s("url"), s("https://github.com/KvanderTech/ck-launcher")}});
        },
        this);
    updateActions->addStretch();
    auto *note =
        new QLabel(tr("Данные прежней версии используются автоматически. Перед обновлением базы "
                      "создаётся резервная копия.\nWindows 7/8.1 используют отдельную сборку "
                      "Legacy на Qt 5; доступность игры зависит также от Java и видеодрайвера."));
    note->setWordWrap(true);
    note->setProperty("muted", true);
    layout->addWidget(note);
    return page;
}
void LauncherWindow::refreshRuntimes() {
    call(s("runtime_statuses"), {}, [this](const QJsonValue &v) {
        auto list = v.toArray();
        javaTable->setRowCount(list.size());
        for (int r = 0; r < list.size(); ++r) {
            auto j = list[r].toObject();
            int major = j.value(s("requirement")).toInt();
            auto state = value(j, "state");
            cells(javaTable, r,
                  {QString::number(major),
                   state == s("valid")     ? tr("Готова")
                   : state == s("missing") ? tr("Не найдена")
                                           : tr("Требует проверки"),
                   value(j, "path")});
            javaTable->item(r, 0)->setData(Qt::UserRole, major);
        }
        if (javaTable->currentRow() < 0 && !list.isEmpty())
            javaTable->selectRow(0);
    });
}
void LauncherWindow::browseFiles() {
    if (selectedBuild.isEmpty())
        return;
    const auto build = selectedBuild;
    auto *dialog = new QDialog(this);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(tr("Файлы сборки"));
    dialog->resize(760, 540);
    auto *layout = new QVBoxLayout(dialog);
    auto *path = new QLineEdit;
    path->setPlaceholderText(tr("Путь внутри сборки, например saves"));
    layout->addWidget(path);
    auto *files = table({tr("Имя"), tr("Тип"), tr("Размер, байт")}, layout);
    auto refresh = [this, dialog, files, path, build] {
        QPointer<QDialog> guard(dialog);
        core->request(s("list_build_files"),
                      {{s("buildId"), build}, {s("relativePath"), path->text()}},
                      [this, guard, files](const QJsonValue &v, const QJsonObject &e) {
                          if (!guard)
                              return;
                          if (!e.isEmpty()) {
                              message(value(e, "message"), true);
                              return;
                          }
                          auto list = v.toArray();
                          files->setRowCount(list.size());
                          for (int r = 0; r < list.size(); ++r) {
                              auto f = list[r].toObject();
                              cells(files, r,
                                    {value(f, "name"), value(f, "kind"),
                                     QString::number(f.value(s("size")).toDouble(), 'f', 0)});
                              files->item(r, 0)->setData(Qt::UserRole, f);
                          }
                      });
    };
    connect(path, &QLineEdit::returnPressed, dialog, refresh);
    connect(files, &QTableWidget::cellDoubleClicked, dialog, [files, path, refresh](int row, int) {
        auto item = files->item(row, 0)->data(Qt::UserRole).toJsonObject();
        if (value(item, "kind") == s("directory")) {
            path->setText(value(item, "relativePath"));
            refresh();
        }
    });
    auto *actions = new QHBoxLayout;
    layout->addLayout(actions);
    button(tr("Обновить"), actions, refresh, dialog);
    button(
        tr("В корень"), actions,
        [path, refresh] {
            path->clear();
            refresh();
        },
        dialog);
    button(
        tr("Показать в Проводнике"), actions,
        [this, build, files, path] {
            auto relative = path->text();
            if (files->currentRow() >= 0)
                relative =
                    value(files->item(files->currentRow(), 0)->data(Qt::UserRole).toJsonObject(),
                          "relativePath");
            call(s("open_build_path"), {{s("buildId"), build}, {s("relativePath"), relative}});
        },
        dialog);
    refresh();
    dialog->show();
}
QWidget *LauncherWindow::logsPage() {
    auto *page = new QWidget;
    auto *layout = pageLayout(page, tr("Что происходит с игрой"),
                              tr("Показывается ограниченный фрагмент журнала. Типичные токены "
                                 "авторизации скрываются перед выводом и экспортом."));
    auto *actions = new QHBoxLayout;
    layout->addLayout(actions);
    logFiles = new QComboBox;
    actions->addWidget(logFiles, 1);
    button(tr("Обновить"), actions, [this] { showLogs(); }, this);
    button(
        tr("Экспорт…"), actions,
        [this] {
            auto name = QFileDialog::getSaveFileName(this, tr("Сохранить журнал"),
                                                     s("minecraft-log.txt"), tr("Текст (*.txt)"));
            if (name.isEmpty())
                return;
            QSaveFile file(name);
            if (!file.open(QIODevice::WriteOnly) ||
                file.write(logText->toPlainText().toUtf8()) < 0 || !file.commit())
                message(tr("Не удалось сохранить журнал."), true);
        },
        this);
    logText = new QPlainTextEdit;
    logText->setReadOnly(true);
    logText->setMaximumBlockCount(15000);
    layout->addWidget(logText, 1);
    connect(logFiles, qOverload<int>(&QComboBox::currentIndexChanged), this, [this](int index) {
        if (index < 0)
            return;
        const auto path = logFiles->currentData().toString();
        call(path.isEmpty() ? s("read_latest_game_log") : s("read_build_log"),
             path.isEmpty() ? QJsonObject{}
                            : QJsonObject{{s("buildId"), selectedBuild}, {s("relativePath"), path}},
             [this](const QJsonValue &v) { logText->setPlainText(v.toString()); });
    });
    return page;
}
void LauncherWindow::showLogs() {
    logFiles->clear();
    logFiles->addItem(tr("Последний журнал запуска"), QString());
    if (selectedBuild.isEmpty())
        return;
    const auto build = selectedBuild;
    call(s("list_build_logs"), {{s("buildId"), build}}, [this, build](const QJsonValue &v) {
        if (build != selectedBuild)
            return;
        for (const auto &item : v.toArray()) {
            auto f = item.toObject();
            logFiles->addItem(value(f, "name"), value(f, "relativePath"));
        }
    });
}
