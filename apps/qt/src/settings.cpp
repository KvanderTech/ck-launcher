#include "window.h"
QWidget *LauncherWindow::settingsPage() {
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(76, 44, 76, 24);
    layout->setSpacing(20);
    layout->addWidget(label(tr("Настройки"), "heading"));
    auto *columns = new QHBoxLayout;
    columns->setSpacing(16);
    auto *left = new QVBoxLayout;
    left->setSpacing(12);
    auto *ram = panel();
    ram->setMaximumWidth(360);
    ram->setMinimumWidth(280);
    auto *r = new QVBoxLayout(ram);
    r->setContentsMargins(18, 18, 18, 18);
    r->setSpacing(12);
    r->addWidget(label(tr("Оперативная память"), "strong"));
    memory = new QSpinBox;
    memory->setObjectName(s("memory"));
    memory->setRange(1024, 65536);
    memory->setSingleStep(512);
    memory->setSuffix(tr(" МБ"));
    memory->setButtonSymbols(QAbstractSpinBox::NoButtons);
    r->addWidget(memory);
    memorySlider = new QSlider(Qt::Horizontal);
    memorySlider->setRange(1024, 65536);
    memorySlider->setSingleStep(512);
    memorySlider->setPageStep(1024);
    r->addWidget(memorySlider);
    auto *save = button(
        tr("Сохранить"), r,
        [this] {
            const auto requested = memory->value();
            call(
                s("update_profile_memory"), {{s("memoryMb"), requested}},
                [this, requested](const QJsonValue &v) {
                    profile = v.toObject();
                    if (memory->value() == requested)
                        findChild<QPushButton *>(s("save-memory"))->hide();
                },
                true);
        },
        this);
    save->setObjectName(s("save-memory"));
    save->hide();
    connect(memory, qOverload<int>(&QSpinBox::valueChanged), this, [this, save](int n) {
        QSignalBlocker blocker(memorySlider);
        memorySlider->setValue(n);
        save->setVisible(n != profile.value(s("memoryMb")).toInt());
    });
    connect(memorySlider, &QSlider::valueChanged, this, [this](int n) {
        const int step = memory->singleStep();
        memory->setValue(
            qBound(memory->minimum(), (n + step / 2) / step * step, memory->maximum()));
    });
    left->addWidget(ram);
    auto *directory = panel();
    auto *d = new QVBoxLayout(directory);
    d->setContentsMargins(18, 18, 18, 18);
    d->setSpacing(12);
    d->addWidget(label(tr("Папка игры"), "strong"));
    gameDirectory = label(QString(), "mutedSmall");
    gameDirectory->setWordWrap(true);
    gameDirectory->setTextInteractionFlags(Qt::TextSelectableByMouse);
    gameDirectory->setMaximumWidth(322);
    d->addWidget(gameDirectory);
    button(
        tr("Выбрать папку игры"), d,
        [this] {
            call(
                s("choose_game_directory"), {},
                [this](const QJsonValue &v) {
                    if (v.isObject()) {
                        profile = v.toObject();
                        gameDirectory->setText(value(profile, "gameDir"));
                        refreshLibrary();
                    }
                },
                true);
        },
        this);
    left->addWidget(directory);
    auto *updates = panel();
    auto *u = new QVBoxLayout(updates);
    u->setContentsMargins(18, 18, 18, 18);
    u->setSpacing(10);
    u->addWidget(label(tr("Обновление лаунчера"), "strong"));
    updateStatus = label(tr("Готово к проверке"), "mutedSmall");
    updateStatus->setWordWrap(true);
    u->addWidget(updateStatus);
    button(
        tr("Проверить обновления"), u,
        [this] {
            updateStatus->setText(tr("Проверяем…"));
            core->request(s("check_update"), {}, [this](const QJsonValue &v, const QJsonObject &e) {
                if (!e.isEmpty()) {
                    updateStatus->setText(value(e, "message"));
                    return;
                }
                const auto update = v.toObject();
                if (!update.value(s("available")).toBool()) {
                    updateStatus->setText(tr("Установлена актуальная версия"));
                    return;
                }
                updateStatus->setText(tr("Доступна версия %1").arg(value(update, "version")));
                QMessageBox info(this);
                info.setWindowTitle(tr("Доступно обновление"));
                info.setTextFormat(Qt::PlainText);
                info.setText(tr("Версия %1\n%2\n\nСкачать и установить обновление?")
                                 .arg(value(update, "version"), value(update, "notes")));
                info.setStandardButtons(QMessageBox::Yes | QMessageBox::No);
                if (info.exec() == QMessageBox::Yes) {
                    updateStatus->setText(tr("Скачиваем и проверяем подпись…"));
                    call(
                        s("install_update"),
                        {{s("assetUrl"), value(update, "assetUrl")},
                         {s("signatureUrl"), value(update, "signatureUrl")},
                         {s("installDir"), QCoreApplication::applicationDirPath()},
                         {s("launcherPid"), qint64(QCoreApplication::applicationPid())}},
                        [this](const QJsonValue &) {
                            updateStatus->setText(tr("Обновление готово. Перезапускаем…"));
                            QTimer::singleShot(250, qApp, &QCoreApplication::quit);
                        },
                        true);
                }
            });
        },
        this);
    left->addWidget(updates);
    auto *appearance = panel();
    auto *a = new QVBoxLayout(appearance);
    a->setContentsMargins(18, 18, 18, 18);
    a->addWidget(label(tr("Интерфейс и звуки"), "strong"));
    auto *sounds = new QCheckBox(tr("Звуки лаунчера"));
    sounds->setObjectName(s("sounds-setting"));
    sounds->setChecked(AudioFeedback::isEnabled());
    sounds->setToolTip(tr("Нажатия, установка, запуск и завершение игры"));
    a->addWidget(sounds);
    connect(sounds, &QCheckBox::toggled, this,
            [](bool enabled) { AudioFeedback::setEnabled(enabled); });
    auto *motion = new QCheckBox(tr("Плавные анимации"));
    motion->setObjectName(s("motion-setting"));
    motion->setChecked(QSettings().value(s("motion"), true).toBool());
    a->addWidget(motion);
    connect(motion, &QCheckBox::toggled, this, [this](bool enabled) {
        QSettings().setValue(s("motion"), enabled);
        qApp->setProperty("reduceMotion", !enabled);
        background->setMotion(enabled);
        skinPreview->setAnimated(enabled);
    });
    left->addWidget(appearance);
    left->addStretch();
    columns->addLayout(left, 36);
    auto *java = panel();
    auto *j = new QVBoxLayout(java);
    j->setContentsMargins(18, 18, 18, 18);
    j->setSpacing(12);
    auto *heading = new QHBoxLayout;
    heading->addWidget(label(tr("Установки Java"), "strong"), 1);
    auto *refresh = iconButton(s("dots"), tr("Обновить установки Java и версии Minecraft"));
    refresh->setFixedSize(24, 24);
    refresh->setIconSize(QSize(18, 18));
    connect(refresh, &QPushButton::clicked, this, [this] {
        refreshRuntimes();
        loadVersions();
    });
    heading->addWidget(refresh);
    j->addLayout(heading);
    auto *hint =
        label(tr("Лаунчер подберёт подходящую Java для выбранной версии игры."), "mutedSmall");
    hint->setWordWrap(true);
    j->addWidget(hint);
    runtimeRows = new QVBoxLayout;
    runtimeRows->setSpacing(10);
    j->addLayout(runtimeRows);
    columns->addWidget(java, 66, Qt::AlignTop);
    layout->addLayout(columns);
    layout->addStretch();
    return scrollPage(page);
}
void LauncherWindow::refreshRuntimes() {
    core->request(s("runtime_statuses"), {}, [this](const QJsonValue &v, const QJsonObject &e) {
        clearLayout(runtimeRows);
        if (!e.isEmpty()) {
            auto *hint = label(value(e, "message"), "muted");
            hint->setWordWrap(true);
            runtimeRows->addWidget(hint);
            return;
        }
        for (const auto &item : v.toArray()) {
            const auto runtime = item.toObject();
            const auto major = runtime.value(s("requirement")).toInt();
            const auto state = value(runtime, "state");
            auto *card = panel(s("inset"));
            auto *layout = new QVBoxLayout(card);
            layout->setContentsMargins(12, 12, 12, 12);
            layout->setSpacing(10);
            auto *head = new QHBoxLayout;
            head->addWidget(label(tr("Java %1").arg(major), "strong"), 1);
            auto *badge = label(state == s("valid")     ? tr("Готова")
                                : state == s("missing") ? tr("Не установлена")
                                                        : tr("Не подходит"),
                                "statusBadge");
            badge->setProperty("valid", state == s("valid"));
            head->addWidget(badge);
            layout->addLayout(head);
            auto *path = new QLineEdit(value(runtime, "path"));
            path->setReadOnly(true);
            path->setToolTip(value(runtime, "path"));
            path->setPlaceholderText(tr("Выберите или установите Java"));
            layout->addWidget(path);
            auto *actions = new QHBoxLayout;
            for (const auto &pair :
                 QList<QPair<QString, QString>>{{tr("Найти"), s("detect_runtime")},
                                                {tr("Выбрать"), s("choose_runtime_path")},
                                                {tr("Установить"), s("install_runtime")}}) {
                auto *b = button(
                    pair.first, actions,
                    [this, major, method = pair.second] {
                        call(
                            method, {{s("requirement"), major}},
                            [this](const QJsonValue &) { refreshRuntimes(); }, true);
                    },
                    this);
                b->setProperty("compact", true);
                b->setObjectName(pair.second + s("-") + QString::number(major));
            }
            actions->addStretch();
            layout->addLayout(actions);
            runtimeRows->addWidget(card);
        }
    });
}
QWidget *LauncherWindow::logsPage() {
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(12);
    auto *actions = new QHBoxLayout;
    logFiles = new QComboBox;
    actions->addWidget(logFiles, 1);
    button(tr("Обновить"), actions, [this] { showLogs(); }, this);
    button(
        tr("Сохранить журнал"), actions,
        [this] {
            auto name = QFileDialog::getSaveFileName(this, tr("Сохранить журнал"),
                                                     s("minecraft-log.txt"), tr("Текст (*.txt)"));
            if (name.isEmpty())
                return;
            QSaveFile file(name);
            const auto data = logText->toPlainText().toUtf8();
            if (!file.open(QIODevice::WriteOnly) || file.write(data) != data.size() ||
                !file.commit())
                message(tr("Не удалось сохранить журнал."), true);
        },
        this);
    layout->addLayout(actions);
    logText = new QPlainTextEdit;
    logText->setReadOnly(true);
    logText->setMaximumBlockCount(15000);
    logText->setMinimumHeight(330);
    layout->addWidget(logText, 1);
    connect(logFiles, qOverload<int>(&QComboBox::currentIndexChanged), this, [this](int index) {
        if (index < 0)
            return;
        const auto path = logFiles->currentData().toString(), build = selectedBuild;
        logText->clear();
        call(path.isEmpty() ? s("read_latest_game_log") : s("read_build_log"),
             path.isEmpty() ? QJsonObject{}
                            : QJsonObject{{s("buildId"), build}, {s("relativePath"), path}},
             [this, build, path](const QJsonValue &v) {
                 if (build == selectedBuild && path == logFiles->currentData().toString())
                     logText->setPlainText(v.toString());
             });
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
            const auto file = item.toObject();
            logFiles->addItem(value(file, "name"), value(file, "relativePath"));
        }
    });
}
