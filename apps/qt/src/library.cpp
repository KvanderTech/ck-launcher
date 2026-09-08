#include "window.h"
QWidget *LauncherWindow::libraryPage() {
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(40, 44, 44, 24);
    layout->setSpacing(20);
    auto *head = new QHBoxLayout;
    head->addWidget(label(tr("Библиотека"), "heading"), 1);
    button(tr("+ Создать сборку"), head, [this] { createBuild(); }, this);
    button(
        tr("+ Найти сборку"), head,
        [this] {
            catalogKind = s("modpack");
            navigate(2);
            searchCatalog();
        },
        this, true);
    layout->addLayout(head);
    libraryCards = new CardGrid(500, 84, 2);
    layout->addWidget(libraryCards);
    libraryHint = label(tr("Загружаем библиотеку…"), "muted");
    libraryHint->setWordWrap(true);
    layout->addWidget(libraryHint);
    layout->addStretch();
    return scrollPage(page);
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
        bool found = false;
        for (const auto &b : builds)
            if (value(b.toObject(), "id") == selectedBuild)
                found = true;
        if (!found) {
            selectedBuild.clear();
            for (const auto &b : builds)
                if (b.toObject().value(s("isActive")).toBool())
                    selectedBuild = value(b.toObject(), "id");
            if (selectedBuild.isEmpty() && !builds.isEmpty())
                selectedBuild = value(builds[0].toObject(), "id");
        }
        renderLibrary();
        refreshContent();
    });
}
void LauncherWindow::renderLibrary() {
    libraryCards->clear();
    clearLayout(sidebarBuilds);
    clearLayout(catalogBuilds);
    libraryHint->setText(builds.isEmpty()
                             ? tr("Пока нет сборок. Создайте свою или найдите готовую в каталоге.")
                             : QString());
    libraryHint->setVisible(builds.isEmpty());
    QSignalBlocker block(buildFilter);
    buildFilter->clear();
    for (const auto &item : builds) {
        auto b = item.toObject();
        const auto id = value(b, "id");
        auto *card = clickPanel([this, id] { openBuild(id); });
        card->setProperty("selected", b.value(s("isActive")).toBool());
        card->setObjectName(s("build-card-") + id);
        auto *row = new QHBoxLayout(card);
        row->setContentsMargins(12, 10, 12, 10);
        row->setSpacing(12);
        auto *icon = new Picture(50);
        icon->setFallback(value(b, "name"));
        row->addWidget(icon);
        images->load(value(b, "iconUrl"), icon,
                     [icon](const QImage &image) { icon->setImage(image); });
        auto *copy = new QVBoxLayout;
        copy->setSpacing(2);
        auto *title = new MotionButton(value(b, "name"));
        title->setProperty("textButton", true);
        title->setProperty("strong", true);
        title->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
        title->setToolTip(tr("Открыть сборку"));
        connect(title, &QPushButton::clicked, this, [this, id] { openBuild(id); });
        copy->addWidget(title);
        QString loader = value(b, "loader");
        if (!loader.isEmpty())
            loader[0] = loader[0].toUpper();
        copy->addWidget(label(loader + s(" · ") + baseGameVersion(b), "small"));
        if (b.value(s("isActive")).toBool())
            copy->addWidget(label(tr("Текущая сборка"), "accentSmall"));
        row->addLayout(copy, 1);
        auto *run = button(
            tr("Играть"), row,
            [this, id] {
                selectedBuild = id;
                launch();
            },
            this, true);
        run->setProperty("playAction", true);
        run->setFixedSize(92, 38);
        libraryCards->append(card);
        auto *quick = new MotionButton;
        quick->setObjectName(s("build-shortcut-") + id);
        quick->setFixedSize(44, 44);
        quick->setProperty("buildShortcut", true);
        quick->setProperty("selected", b.value(s("isActive")).toBool());
        quick->setToolTip(value(b, "name"));
        quick->setAccessibleName(value(b, "name"));
        quick->setIconSize(QSize(36, 36));
        quick->setText(value(b, "name").left(1));
        quick->setCursor(Qt::PointingHandCursor);
        sidebarBuilds->addWidget(quick, 0, Qt::AlignHCenter);
        images->load(value(b, "iconUrl"), quick, [quick](const QImage &image) {
            if (!image.isNull()) {
                quick->setIcon(roundedIcon(image));
                quick->setText({});
            }
        });
        connect(quick, &QPushButton::clicked, this, [this, id] { openBuild(id); });
        auto *chip = new MotionButton;
        chip->setProperty("buildChip", true);
        chip->setProperty("selected", id == selectedBuild);
        chip->setFixedSize(218, 62);
        auto *chipLayout = new QHBoxLayout(chip);
        chipLayout->setContentsMargins(12, 8, 12, 8);
        chipLayout->setSpacing(10);
        auto *chipIcon = new Picture(38);
        chipIcon->setFallback(value(b, "name"));
        chipIcon->setAttribute(Qt::WA_TransparentForMouseEvents);
        chipLayout->addWidget(chipIcon);
        images->load(value(b, "iconUrl"), chipIcon,
                     [chipIcon](const QImage &i) { chipIcon->setImage(i); });
        auto *chipCopy = new QVBoxLayout;
        chipCopy->setSpacing(2);
        auto *chipTitle = label(value(b, "name"), "chipTitle");
        chipTitle->setAttribute(Qt::WA_TransparentForMouseEvents);
        chipTitle->setMaximumWidth(145);
        chipCopy->addWidget(chipTitle);
        auto *chipVersion = label(value(b, "loader") + s(" · ") + baseGameVersion(b), "mutedSmall");
        chipVersion->setAttribute(Qt::WA_TransparentForMouseEvents);
        chipCopy->addWidget(chipVersion);
        chipLayout->addLayout(chipCopy, 1);
        chip->setToolTip(value(b, "name"));
        catalogBuilds->addWidget(chip);
        connect(chip, &QPushButton::clicked, this, [this, id] {
            buildFilter->setCurrentIndex(buildFilter->findData(id));
            renderLibrary();
        });
        buildFilter->addItem(value(b, "name"), id);
        if (id == selectedBuild)
            buildFilter->setCurrentIndex(buildFilter->count() - 1);
    }
    catalogBuilds->addStretch();
    if (auto *scroll = findChild<QScrollArea *>(s("catalog-builds-scroll")))
        scroll->setVisible(!builds.isEmpty());
    if (auto *scroll = findChild<QScrollArea *>(s("sidebar-builds-scroll"))) {
        // Allow the sidebar to shrink on shorter displays; its content can scroll.
        scroll->setMaximumHeight(qMin(278, int(builds.size()) * 52));
        scroll->setMinimumHeight(0);
        scroll->setVisible(!builds.isEmpty());
    }
    auto b = currentBuild();
    detailName->setText(value(b, "name"));
    detailIcon->setFallback(value(b, "name"));
    images->load(value(b, "iconUrl"), detailIcon, [this, id = selectedBuild](const QImage &image) {
        if (id == selectedBuild)
            detailIcon->setImage(image);
    });
    gameDirectory->setText(value(b, "gameDir").isEmpty() ? value(profile, "gameDir")
                                                         : value(b, "gameDir"));
    updatePlayState();
}
void LauncherWindow::openBuild(const QString &id) {
    AudioFeedback::play(s("build-switch"));
    selectedBuild = id;
    filePath->clear();
    filesTable->setRowCount(0);
    if (auto *tabs = findChild<QTabBar *>(s("buildTabs")))
        tabs->setCurrentIndex(0);
    renderLibrary();
    refreshContent();
    navigate(5);
    if (!busy && !running && operationId.isEmpty())
        call(
            s("select_build"), {{s("buildId"), id}},
            [this](const QJsonValue &v) {
                profile = v.toObject();
                refreshLibrary();
            },
            true);
}
void LauncherWindow::refreshContent() {
    const auto id = selectedBuild;
    installed = {};
    renderContent();
    if (id.isEmpty()) {
        installed = {};
        renderContent();
        return;
    }
    call(s("list_installed_content"), {{s("buildId"), id}}, [this, id](const QJsonValue &v) {
        if (id != selectedBuild)
            return;
        installed = v.toArray();
        renderContent();
        if (currentPage == 2)
            renderCatalog();
    });
}
void LauncherWindow::renderContent() {
    if (auto *tabs = findChild<QTabBar *>(s("buildTabs")))
        tabs->setTabText(0, tr("Контент %1").arg(installed.size()));
    clearLayout(contentRows);
    int visible = 0;
    for (const auto &item : installed) {
        auto content = item.toObject();
        auto type = value(content, "projectType");
        if (!contentKind.isEmpty() && contentKind != type)
            continue;
        ++visible;
        auto openProject = [this, content, type] {
            const auto id = value(content, "projectId");
            if (value(content, "source") == s("local") || id.isEmpty())
                return;
            projectDetails({{s("project_id"), id},
                            {s("title"), value(content, "title")},
                            {s("project_type"), type},
                            {s("icon_url"), value(content, "iconUrl")}});
        };
        auto *card = clickPanel(openProject);
        card->setObjectName(s("content-card-") + value(content, "projectId"));
        auto *row = new QHBoxLayout(card);
        row->setContentsMargins(16, 12, 16, 12);
        row->setSpacing(14);
        auto *icon = new Picture(52);
        icon->setFallback(value(content, "title"));
        row->addWidget(icon);
        images->load(value(content, "iconUrl"), icon,
                     [icon](const QImage &i) { icon->setImage(i); });
        auto *copy = new QVBoxLayout;
        copy->setSpacing(3);
        auto *title = new MotionButton(value(content, "title"));
        title->setProperty("textButton", true);
        title->setProperty("strong", true);
        connect(title, &QPushButton::clicked, this, openProject);
        copy->addWidget(title);
        QString kind = type == s("mod")            ? tr("Мод")
                       : type == s("resourcepack") ? tr("Ресурспак")
                       : type == s("shader")       ? tr("Шейдер")
                                                   : tr("Модпак");
        copy->addWidget(label(kind, "small"));
        auto *filename = label(value(content, "filename"), "small");
        filename->setWordWrap(true);
        copy->addWidget(filename);
        row->addLayout(copy, 1);
        const auto build = selectedBuild, project = value(content, "projectId");
        bool enabled = content.value(s("enabled")).toBool();
        auto *toggle = button(
            enabled ? tr("Включён") : tr("Выключен"), row,
            [this, build, project, enabled] {
                call(
                    s("set_installed_content_enabled"),
                    {{s("buildId"), build}, {s("projectId"), project}, {s("enabled"), !enabled}},
                    [this](const QJsonValue &) { refreshContent(); }, true);
            },
            this);
        toggle->setProperty("selected", enabled);
        toggle->setEnabled(type != s("modpack"));
        auto *remove = iconButton(s("close"), tr("Удалить содержимое"));
        remove->setProperty("danger", true);
        row->addWidget(remove);
        connect(remove, &QPushButton::clicked, this,
                [this, build, project, title = value(content, "title")] {
                    if (QMessageBox::question(this, tr("Удаление содержимого"),
                                              tr("Удалить «%1»?").arg(title),
                                              QMessageBox::Yes | QMessageBox::No,
                                              QMessageBox::No) == QMessageBox::Yes)
                        call(
                            s("remove_installed_content"),
                            {{s("buildId"), build}, {s("projectId"), project}},
                            [this](const QJsonValue &) { refreshContent(); }, true);
                });
        contentRows->addWidget(card);
    }
    if (!visible)
        contentRows->addWidget(label(
            tr("Здесь пока ничего нет. Добавьте файлы с устройства или из каталога."), "muted"));
    contentRows->addStretch();
}
void LauncherWindow::launch() {
    AudioFeedback::play(s("launch"));
    if (!ready)
        return;
    if (selectedBuild.isEmpty()) {
        navigate(1);
        createBuild();
        return;
    }
    if (selectedAccount.isEmpty()) {
        navigate(3);
        message(tr("Войдите в Microsoft, чтобы запустить лицензионную игру."));
        return;
    }
    call(
        s("select_build"), {{s("buildId"), selectedBuild}},
        [this](const QJsonValue &v) {
            profile = v.toObject();
            call(
                s("launch_or_install"), {{s("profileId"), s("default")}},
                [this](const QJsonValue &v) {
                    const auto id = v.toString();
                    if (completedOperations.contains(id))
                        return;
                    operationId = id;
                    if (running) {
                        updatePlayState();
                        return;
                    }
                    message(tr("Подготавливаем Minecraft. Первое скачивание может занять несколько "
                               "минут."));
                    updatePlayState();
                },
                true);
        },
        true);
}
void LauncherWindow::buildSettings() {
    if (selectedBuild.isEmpty())
        return;
    const auto id = selectedBuild;
    const auto b = currentBuild();
    QMenu menu(this);
    auto *rename = menu.addAction(tr("Переименовать"));
    auto *icon = menu.addAction(tr("Изменить значок"));
    auto *repair = menu.addAction(tr("Восстановить файлы"));
    auto *folder = menu.addAction(tr("Открыть папку"));
    menu.addSeparator();
    auto *remove = menu.addAction(tr("Удалить сборку"));
    auto *chosen = menu.exec(QCursor::pos());
    if (chosen == rename) {
        bool ok = false;
        auto name = QInputDialog::getText(this, tr("Название сборки"), tr("Новое название"),
                                          QLineEdit::Normal, value(b, "name"), &ok);
        if (ok)
            call(
                s("rename_build"), {{s("buildId"), id}, {s("name"), name}},
                [this](const QJsonValue &) { refreshLibrary(); }, true);
    } else if (chosen == icon)
        call(
            s("choose_build_icon"), {{s("buildId"), id}},
            [this](const QJsonValue &) { refreshLibrary(); }, true);
    else if (chosen == repair)
        call(
            s("repair_build"), {{s("buildId"), id}},
            [this](const QJsonValue &) { refreshLibrary(); }, true);
    else if (chosen == folder)
        call(s("open_build_folder"), {{s("buildId"), id}});
    else if (chosen == remove &&
             QMessageBox::question(
                 this, tr("Удалить сборку?"),
                 tr("«%1» вместе с мирами будет перемещена в папку trash лаунчера.")
                     .arg(value(b, "name")),
                 QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes)
        call(
            s("delete_build"), {{s("buildId"), id}},
            [this](const QJsonValue &) {
                selectedBuild.clear();
                refreshLibrary();
                navigate(1);
            },
            true);
}
void LauncherWindow::addLocalContent() {
    if (selectedBuild.isEmpty())
        return;
    bool ok = false;
    auto kind = QInputDialog::getItem(this, tr("Добавить с устройства"), tr("Тип содержимого"),
                                      {tr("Мод .jar"), tr("Ресурспак .zip"), tr("Шейдер .zip")}, 0,
                                      false, &ok);
    if (!ok)
        return;
    const auto type = kind.startsWith(tr("Мод"))      ? s("mod")
                      : kind.startsWith(tr("Ресурс")) ? s("resourcepack")
                                                      : s("shader");
    if (type == s("mod") &&
        QMessageBox::question(this, tr("Доверие к моду"),
                              tr("Мод выполняет код на компьютере. Вы доверяете его автору?"),
                              QMessageBox::Yes | QMessageBox::No,
                              QMessageBox::No) != QMessageBox::Yes)
        return;
    call(
        s("import_local_content"), {{s("buildId"), selectedBuild}, {s("projectType"), type}},
        [this](const QJsonValue &) { refreshContent(); }, true);
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
    loader->addItems({s("vanilla"), s("fabric"), s("quilt"), s("forge")});
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
                navigate(1);
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
                    navigate(1);
                },
                true);
    });
}
