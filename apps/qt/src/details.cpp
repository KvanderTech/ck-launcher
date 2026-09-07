#include "window.h"
namespace {
class ContentStack final : public QStackedWidget {
  public:
    ContentStack() {
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
        connect(this, &QStackedWidget::currentChanged, this, [this] { updateGeometry(); });
    }
    QSize sizeHint() const override {
        return currentWidget() ? currentWidget()->sizeHint() : QSize();
    }
    QSize minimumSizeHint() const override {
        return currentWidget() ? currentWidget()->minimumSizeHint() : QSize();
    }
};
} // namespace
QWidget *LauncherWindow::detailPage() {
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(40, 36, 44, 24);
    layout->setSpacing(16);
    auto *backRow = new QHBoxLayout;
    auto *back = button(tr("← Библиотека"), backRow, [this] { navigate(1); }, this);
    back->setObjectName(s("back-to-library"));
    backRow->addStretch();
    layout->addLayout(backRow);
    auto *hero = panel(s("hero"));
    auto *header = new QHBoxLayout(hero);
    header->setContentsMargins(22, 23, 22, 23);
    header->setSpacing(16);
    detailIcon = new Picture(66);
    header->addWidget(detailIcon);
    detailName = label(QString(), "detailHeading");
    header->addWidget(detailName, 1);
    detailPlay = button(tr("Играть"), header, [this] { launch(); }, this, true);
    detailPlay->setIcon(glyph(s("play"), Qt::white, 16));
    detailPlay->setProperty("playAction", true);
    detailPlay->setFixedSize(128, 48);
    auto *settings = iconButton(s("settings"), tr("Настройки сборки"));
    header->addWidget(settings);
    connect(settings, &QPushButton::clicked, this, &LauncherWindow::buildSettings);
    layout->addWidget(hero);
    auto *tabs = new QTabBar;
    tabs->setObjectName(s("buildTabs"));
    tabs->setExpanding(false);
    tabs->setDrawBase(false);
    for (const auto &title : {tr("Контент"), tr("Файлы"), tr("Миры"), tr("Логи")})
        tabs->addTab(title);
    layout->addWidget(tabs, 0, Qt::AlignLeft);
    detailSections = new ContentStack;
    layout->addWidget(detailSections);
    auto *content = new QWidget;
    auto *c = new QVBoxLayout(content);
    c->setContentsMargins(0, 0, 0, 0);
    c->setSpacing(12);
    auto *tools = new QHBoxLayout;
    auto *group = new QButtonGroup(content);
    const QStringList titles{tr("Всё"), tr("Моды"), tr("Ресурспаки"), tr("Шейдеры")},
        kinds{QString(), s("mod"), s("resourcepack"), s("shader")};
    for (int i = 0; i < titles.size(); ++i) {
        auto *b = button(
            titles[i], tools,
            [this, kind = kinds[i]] {
                contentKind = kind;
                renderContent();
            },
            this);
        b->setCheckable(true);
        b->setChecked(i == 0);
        group->addButton(b);
    }
    tools->addStretch();
    button(tr("↑ С устройства"), tools, [this] { addLocalContent(); }, this);
    button(
        tr("+ Каталог"), tools,
        [this] {
            catalogKind = s("mod");
            navigate(2);
            searchCatalog();
        },
        this, true);
    c->addLayout(tools);
    auto *rows = new QWidget;
    contentRows = new QVBoxLayout(rows);
    contentRows->setContentsMargins(0, 0, 0, 0);
    contentRows->setSpacing(10);
    c->addWidget(rows);
    detailSections->addWidget(content);
    auto *files = new QWidget;
    auto *f = new QVBoxLayout(files);
    f->setContentsMargins(0, 0, 0, 0);
    auto *fileTools = new QHBoxLayout;
    filePath = new QLineEdit;
    filePath->setPlaceholderText(tr("Корневая папка сборки"));
    fileTools->addWidget(filePath, 1);
    button(
        tr("В корень"), fileTools,
        [this] {
            filePath->clear();
            refreshFiles();
        },
        this);
    button(tr("Обновить"), fileTools, [this] { refreshFiles(); }, this);
    button(
        tr("Открыть папку"), fileTools,
        [this] {
            call(s("open_build_path"),
                 {{s("buildId"), selectedBuild}, {s("relativePath"), filePath->text()}});
        },
        this);
    f->addLayout(fileTools);
    filesTable = table({tr("Название"), tr("Тип"), tr("Размер")}, f);
    filesTable->setMinimumHeight(330);
    filesTable->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
    connect(filePath, &QLineEdit::returnPressed, this, &LauncherWindow::refreshFiles);
    connect(filesTable, &QTableWidget::cellDoubleClicked, this, [this](int row, int) {
        auto data = filesTable->item(row, 0)->data(Qt::UserRole).toJsonObject();
        if (value(data, "kind") == s("directory")) {
            filePath->setText(value(data, "relativePath"));
            refreshFiles();
        } else
            call(s("open_build_path"),
                 {{s("buildId"), selectedBuild}, {s("relativePath"), value(data, "relativePath")}});
    });
    detailSections->addWidget(files);
    auto *worlds = new QWidget;
    worldRows = new QVBoxLayout(worlds);
    worldRows->setContentsMargins(0, 0, 0, 0);
    worldRows->setSpacing(12);
    detailSections->addWidget(worlds);
    detailSections->addWidget(logsPage());
    connect(tabs, &QTabBar::currentChanged, this, [this](int i) {
        detailSections->setCurrentIndex(i);
        if (i == 1)
            refreshFiles();
        else if (i == 2)
            refreshWorlds();
        else if (i == 3)
            showLogs();
    });
    layout->addStretch();
    return scrollPage(page);
}
void LauncherWindow::browseFiles() {
    navigate(5);
    if (auto *tabs = findChild<QTabBar *>(s("buildTabs")))
        tabs->setCurrentIndex(1);
    refreshFiles();
}
void LauncherWindow::refreshFiles() {
    if (selectedBuild.isEmpty())
        return;
    const auto id = selectedBuild, path = filePath->text();
    call(s("list_build_files"), {{s("buildId"), id}, {s("relativePath"), path}},
         [this, id, path](const QJsonValue &v) {
             if (id != selectedBuild || path != filePath->text())
                 return;
             auto list = v.toArray();
             filesTable->setRowCount(list.size());
             for (int i = 0; i < list.size(); ++i) {
                 auto entry = list[i].toObject();
                 bool directory = value(entry, "kind") == s("directory");
                 cells(filesTable, i,
                       {value(entry, "name"), directory ? tr("Папка") : tr("Файл"),
                        directory ? QString()
                                  : QLocale().formattedDataSize(
                                        qint64(entry.value(s("size")).toDouble()))});
                 filesTable->item(i, 0)->setData(Qt::UserRole, entry);
             }
         });
}
void LauncherWindow::refreshWorlds() {
    clearLayout(worldRows);
    if (selectedBuild.isEmpty())
        return;
    const auto id = selectedBuild;
    call(s("list_build_worlds"), {{s("buildId"), id}}, [this, id](const QJsonValue &v) {
        if (id != selectedBuild)
            return;
        clearLayout(worldRows);
        for (const auto &item : v.toArray()) {
            auto world = item.toObject();
            auto *card = panel();
            auto *row = new QHBoxLayout(card);
            row->setContentsMargins(18, 16, 18, 16);
            auto *copy = new QVBoxLayout;
            copy->addWidget(label(value(world, "name"), "strong"));
            copy->addWidget(label(
                QLocale().formattedDataSize(qint64(world.value(s("size")).toDouble())), "small"));
            row->addLayout(copy, 1);
            button(
                tr("Открыть папку"), row,
                [this, id, path = value(world, "relativePath")] {
                    call(s("open_build_path"), {{s("buildId"), id}, {s("relativePath"), path}});
                },
                this);
            worldRows->addWidget(card);
        }
        if (v.toArray().isEmpty())
            worldRows->addWidget(label(tr("Сохранённых миров пока нет."), "muted"));
        worldRows->addStretch();
    });
}
