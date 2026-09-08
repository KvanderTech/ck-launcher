#include "window.h"
QWidget *LauncherWindow::catalogPage() {
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(40, 44, 44, 24);
    layout->setSpacing(16);
    auto *heading = new QHBoxLayout;
    heading->addWidget(label(tr("Контент и сборки"), "heading"), 1);
    button(tr("Установить .mrpack"), heading, [this] { importPack(); }, this);
    button(tr("+ Создать сборку"), heading, [this] { createBuild(); }, this, true);
    layout->addLayout(heading);
    buildFilter = new QComboBox(page);
    buildFilter->setObjectName(s("catalog-build"));
    buildFilter->hide();
    auto *buildStrip = new QWidget;
    catalogBuilds = new QHBoxLayout(buildStrip);
    catalogBuilds->setContentsMargins(0, 0, 0, 0);
    catalogBuilds->setSpacing(12);
    auto *buildScroll = new QScrollArea;
    buildScroll->setObjectName(s("catalog-builds-scroll"));
    buildScroll->setWidgetResizable(true);
    buildScroll->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    buildScroll->setFixedHeight(70);
    buildScroll->setWidget(buildStrip);
    layout->addWidget(buildScroll);
    connect(buildFilter, qOverload<int>(&QComboBox::currentIndexChanged), this, [this](int index) {
        if (index < 0)
            return;
        selectedBuild = buildFilter->currentData().toString();
        refreshContent();
        const auto b = currentBuild();
        QSignalBlocker a(versionFilter), c(loaderFilter);
        versionFilter->setCurrentIndex(qMax(0, versionFilter->findData(baseGameVersion(b))));
        loaderFilter->setCurrentIndex(qMax(0, loaderFilter->findData(value(b, "loader"))));
        catalogOffset = 0;
        if (ready)
            searchCatalog();
    });
    auto *tabs = new QTabBar;
    tabs->setObjectName(s("catalog-tabs"));
    tabs->setExpanding(false);
    tabs->setDrawBase(false);
    const QStringList kinds{s("modpack"), s("mod"), s("resourcepack"), s("shader")};
    const QStringList names{tr("Сборки"), tr("Моды"), tr("Ресурспаки"), tr("Шейдеры")};
    for (int i = 0; i < names.size(); ++i) {
        tabs->addTab(names[i]);
        tabs->setTabData(i, kinds[i]);
    }
    connect(tabs, &QTabBar::currentChanged, this, [this, tabs](int i) {
        catalogKind = tabs->tabData(i).toString();
        catalogOffset = 0;
        if (ready)
            searchCatalog();
    });
    layout->addWidget(tabs, 0, Qt::AlignLeft);
    auto *search = new QHBoxLayout;
    searchText = new QLineEdit;
    searchText->setObjectName(s("catalog-search"));
    searchText->setPlaceholderText(tr("Поиск: сборки, моды и многое другое…"));
    searchText->setMinimumHeight(48);
    search->addWidget(searchText, 1);
    auto runSearch = [this] {
        catalogOffset = 0;
        searchCatalog();
    };
    button(tr("Найти"), search, runSearch, this, true)->setMinimumHeight(48);
    connect(searchText, &QLineEdit::returnPressed, this, runSearch);
    layout->addLayout(search);
    auto *columns = new QHBoxLayout;
    columns->setSpacing(16);
    auto *results = new QWidget;
    catalogRows = new QVBoxLayout(results);
    catalogRows->setContentsMargins(0, 0, 0, 0);
    catalogRows->setSpacing(10);
    columns->addWidget(results, 1);
    auto *filters = panel();
    filters->setFixedWidth(260);
    auto *f = new QVBoxLayout(filters);
    f->setContentsMargins(18, 18, 18, 18);
    f->setSpacing(12);
    auto *top = new QHBoxLayout;
    top->addWidget(label(tr("Фильтры"), "strong"), 1);
    auto *reset = button(
        tr("Сбросить"), top,
        [this] {
            QSignalBlocker a(versionFilter), b(loaderFilter), c(categoryFilter), d(sortFilter),
                e(hideInstalled);
            versionFilter->setCurrentIndex(0);
            loaderFilter->setCurrentIndex(0);
            categoryFilter->setCurrentIndex(0);
            sortFilter->setCurrentIndex(0);
            hideInstalled->setChecked(false);
            searchText->clear();
            catalogOffset = 0;
            searchCatalog();
        },
        this);
    reset->setProperty("textButton", true);
    reset->setProperty("accentSmall", true);
    f->addLayout(top);
    hideInstalled = new QCheckBox(tr("Скрыть установленное"));
    f->addWidget(hideInstalled);
    auto addFilter = [f](const QString &title) {
        f->addWidget(label(title, "muted"));
        auto *combo = new QComboBox;
        f->addWidget(combo);
        return combo;
    };
    sortFilter = addFilter(tr("Сортировка"));
    sortFilter->addItem(tr("По релевантности"), s("relevance"));
    sortFilter->addItem(tr("По загрузкам"), s("downloads"));
    sortFilter->addItem(tr("Недавно обновлённые"), s("updated"));
    sortFilter->addItem(tr("Новые проекты"), s("newest"));
    versionFilter = addFilter(tr("Версия Minecraft"));
    versionFilter->addItem(tr("Все версии"));
    categoryFilter = addFilter(tr("Категория"));
    categoryFilter->addItem(tr("Все категории"));
    for (const auto &p : QList<QPair<QString, QString>>{{tr("Оптимизация"), s("optimization")},
                                                        {tr("Приключения"), s("adventure")},
                                                        {tr("Технологии"), s("technology")},
                                                        {tr("Магия"), s("magic")},
                                                        {tr("Декор"), s("decoration")},
                                                        {tr("Удобство"), s("utility")}})
        categoryFilter->addItem(p.first, p.second);
    loaderFilter = addFilter(tr("Загрузчик"));
    loaderFilter->addItem(tr("Все загрузчики"));
    for (const auto &loader : {s("fabric"), s("quilt"), s("forge"), s("neoforge")})
        loaderFilter->addItem(loader, loader);
    columns->addWidget(filters, 0, Qt::AlignTop);
    layout->addLayout(columns);
    catalogStatus = label(QString(), "muted");
    catalogStatus->setWordWrap(true);
    layout->addWidget(catalogStatus);
    auto *pagination = new QHBoxLayout;
    auto *prev = button(
        tr("← Назад"), pagination,
        [this] {
            catalogOffset = qMax(0, catalogOffset - 20);
            searchCatalog();
        },
        this);
    prev->setObjectName(s("catalog-prev"));
    prev->setEnabled(false);
    pagination->addStretch();
    auto *next = button(
        tr("Далее →"), pagination,
        [this] {
            catalogOffset += 20;
            searchCatalog();
        },
        this);
    next->setObjectName(s("catalog-next"));
    next->setEnabled(false);
    layout->addLayout(pagination);
    layout->addStretch();
    for (auto *combo : {sortFilter, versionFilter, categoryFilter, loaderFilter})
        connect(combo, qOverload<int>(&QComboBox::currentIndexChanged), this,
                [runSearch](int) { runSearch(); });
    connect(hideInstalled, &QCheckBox::toggled, this, [this] { renderCatalog(); });
    return scrollPage(page);
}
void LauncherWindow::loadVersions() {
    core->request(s("list_game_versions"), {},
                  [this](const QJsonValue &v, const QJsonObject &error) {
                      if (!error.isEmpty())
                          return;
                      versions = {};
                      const auto selection = versionFilter->currentData();
                      QSignalBlocker blocker(versionFilter);
                      versionFilter->clear();
                      versionFilter->addItem(tr("Все версии"));
                      for (const auto &item : v.toArray()) {
                          const auto version = item.toObject();
                          if (value(version, "type") == s("release")) {
                              versions.append(version);
                              versionFilter->addItem(value(version, "id"), value(version, "id"));
                          }
                      }
                      versionFilter->setCurrentIndex(qMax(0, versionFilter->findData(selection)));
                  });
}
void LauncherWindow::searchCatalog() {
    if (!ready)
        return;
    if (auto *tabs = findChild<QTabBar *>(s("catalog-tabs"))) {
        QSignalBlocker blocker(tabs);
        for (int i = 0; i < tabs->count(); ++i)
            if (tabs->tabData(i).toString() == catalogKind)
                tabs->setCurrentIndex(i);
    }
    const auto request = ++catalogRequest;
    catalog = {};
    clearLayout(catalogRows);
    catalogRows->addWidget(label(tr("Загружаем проекты…"), "muted"));
    catalogRows->addStretch();
    findChild<QPushButton *>(s("catalog-prev"))->setEnabled(false);
    findChild<QPushButton *>(s("catalog-next"))->setEnabled(false);
    catalogStatus->setText(tr("Ищем проекты…"));
    auto filter = [](QComboBox *combo) -> QJsonValue {
        return combo->currentData().toString().isEmpty()
                   ? QJsonValue(QJsonValue::Null)
                   : QJsonValue(combo->currentData().toString());
    };
    core->request(
        s("search_modrinth"),
        {{s("query"), searchText->text()},
         {s("projectType"), catalogKind},
         {s("gameVersion"), filter(versionFilter)},
         {s("loader"), filter(loaderFilter)},
         {s("category"), filter(categoryFilter)},
         {s("index"), filter(sortFilter)},
         {s("offset"), catalogOffset}},
        [this, request](const QJsonValue &v, const QJsonObject &error) {
            if (request != catalogRequest)
                return;
            if (!error.isEmpty()) {
                clearLayout(catalogRows);
                catalogRows->addWidget(
                    label(tr("Не удалось загрузить проекты. Повторите поиск."), "muted"));
                catalogRows->addStretch();
                catalogStatus->setText(
                    tr("Каталог недоступен. Проверьте подключение и нажмите «Найти» ещё раз."));
                return;
            }
            const auto result = v.toObject();
            catalog = result.value(s("hits")).toArray();
            renderCatalog();
            const int total = result.value(s("total_hits")).toInt();
            catalogStatus->setText(
                tr("Найдено %1 проектов · страница %2").arg(total).arg(catalogOffset / 20 + 1));
            findChild<QPushButton *>(s("catalog-prev"))->setEnabled(catalogOffset > 0);
            findChild<QPushButton *>(s("catalog-next"))->setEnabled(catalogOffset + 20 < total);
        });
}
void LauncherWindow::renderCatalog() {
    clearLayout(catalogRows);
    int visible = 0;
    for (const auto &item : catalog) {
        const auto project = item.toObject();
        bool isInstalled = false;
        for (const auto &entry : installed)
            if (value(entry.toObject(), "projectId") == value(project, "project_id"))
                isInstalled = true;
        if (hideInstalled->isChecked() && isInstalled)
            continue;
        ++visible;
        auto *card = clickPanel([this, project] { projectDetails(project); });
        card->setAccessibleName(value(project, "title"));
        auto *row = new QHBoxLayout(card);
        row->setContentsMargins(14, 12, 14, 12);
        row->setSpacing(14);
        auto *icon = new Picture(56);
        icon->setFallback(value(project, "title"));
        row->addWidget(icon);
        images->load(value(project, "icon_url"), icon,
                     [icon](const QImage &i) { icon->setImage(i); });
        auto *copy = new QVBoxLayout;
        copy->setSpacing(4);
        auto *title = new MotionButton(value(project, "title"));
        title->setProperty("textButton", true);
        title->setProperty("strong", true);
        title->setToolTip(tr("О проекте"));
        connect(title, &QPushButton::clicked, this, [this, project] { projectDetails(project); });
        copy->addWidget(title);
        auto *description = label(value(project, "description"), "small");
        description->setWordWrap(true);
        description->setMaximumHeight(36);
        description->setMinimumWidth(100);
        copy->addWidget(description);
        QStringList tags;
        for (const auto &tag : project.value(s("categories")).toArray()) {
            if (tags.size() == 4)
                break;
            tags << tag.toString();
        }
        copy->addWidget(label(tr("от %1  ·  %2").arg(value(project, "author"), tags.join(s(" · "))),
                              "mutedSmall"));
        row->addLayout(copy, 1);
        auto *install = button(
            isInstalled ? tr("Установлено") : tr("+ Установить"), row,
            [this, project] { installCatalog(project); }, this);
        install->setEnabled(!isInstalled);
        install->setMinimumWidth(128);
        catalogRows->addWidget(card);
    }
    if (!visible)
        catalogRows->addWidget(label(catalog.isEmpty()
                                         ? tr("Ничего не найдено. Попробуйте изменить фильтры.")
                                         : tr("Все проекты на этой странице уже установлены."),
                                     "muted"));
    catalogRows->addStretch();
}
void LauncherWindow::projectDetails(const QJsonObject &project) {
    projectReturnPage = currentPage == 7 ? 2 : currentPage;
    projectView->open(project, builds, selectedBuild);
    navigate(7);
}
void LauncherWindow::installCatalog(const QJsonObject &project) {
    projectDetails(project);
}
