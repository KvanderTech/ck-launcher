#include "projectview.h"
#include <QDesktopServices>
#include <QTextDocument>

namespace {
// Never let project Markdown read local files or open non-web URL schemes.
class ProjectDocument final : public QTextDocument {
  public:
    ProjectDocument(ImagePool *pool, QObject *parent) : QTextDocument(parent), images(pool) {
        setDefaultStyleSheet(
            s("body{color:#d8e8f4;font-size:14px} "
              "h1,h2,h3{color:#f3f9ff;margin-top:24px} a{color:#62d4ff} "
              "pre,code{background:#112f46} blockquote{color:#9fc2d9} th{background:#16364e}"));
        setDocumentMargin(20);
    }

  protected:
    QVariant loadResource(int type, const QUrl &url) override {
        if (type != QTextDocument::ImageResource || ImagePool::remoteUrl(url.toString()).isEmpty())
            return QImage();
        if (!pending.contains(url)) {
            pending.insert(url);
            QTimer::singleShot(0, this, [this, url] {
                images->load(url.toString(), this, [this, url](const QImage &image) {
                    if (image.isNull())
                        return;
                    addResource(
                        QTextDocument::ImageResource, url,
                        image.scaledToWidth(qMin(image.width(), qMax(200, int(textWidth()) - 44)),
                                            Qt::SmoothTransformation));
                    markContentsDirty(0, characterCount());
                });
            });
        }
        QImage placeholder(1, 1, QImage::Format_ARGB32);
        placeholder.fill(Qt::transparent);
        return placeholder;
    }

  private:
    ImagePool *images;
    QSet<QUrl> pending;
};
QStringList strings(const QJsonArray &array) {
    QStringList result;
    for (const auto &item : array)
        result << item.toString();
    return result;
}
} // namespace

ProjectView::ProjectView(Backend *core, ImagePool *pool, QWidget *parent)
    : QWidget(parent), backend(core), images(pool) {
    setObjectName(s("project-page"));
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(40, 16, 44, 24);
    layout->setSpacing(14);
    auto *top = new QHBoxLayout;
    button(tr("‹ Назад"), top, [this] { emit back(); }, this)->setObjectName(s("project-back"));
    top->addStretch();
    auto *site = button(
        tr("На Modrinth ↗"), top,
        [this] {
            const auto id = value(project, "id");
            if (!id.isEmpty()) {
                QUrl url(s("https://modrinth.com/project/"));
                url.setPath(url.path() + id);
                QDesktopServices::openUrl(url);
            }
        },
        this);
    site->setProperty("textButton", true);
    layout->addLayout(top);
    auto *head = panel(s("hero"));
    auto *h = new QHBoxLayout(head);
    h->setContentsMargins(20, 16, 20, 16);
    icon = new Picture(64);
    h->addWidget(icon);
    auto *copy = new QVBoxLayout;
    title = label(QString(), "sectionTitle");
    title->setWordWrap(true);
    description = label(QString(), "muted");
    description->setWordWrap(true);
    copy->addWidget(title);
    copy->addWidget(description);
    h->addLayout(copy, 1);
    layout->addWidget(head);
    auto *columns = new QHBoxLayout;
    columns->setSpacing(16);
    auto *left = new QVBoxLayout;
    tabs = new QTabBar;
    tabs->setDrawBase(false);
    tabs->setExpanding(false);
    tabs->addTab(tr("Описание"));
    tabs->addTab(tr("Версии"));
    left->addWidget(tabs, 0, Qt::AlignLeft);
    auto *sections = new QStackedWidget;
    body = new QTextBrowser;
    body->setObjectName(s("project-description"));
    body->setOpenLinks(false);
    body->setOpenExternalLinks(false);
    body->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    connect(body, &QTextBrowser::anchorClicked, this, [](const QUrl &url) {
        if ((url.scheme() == s("https") || url.scheme() == s("http")) && url.isValid() &&
            url.userInfo().isEmpty())
            QDesktopServices::openUrl(url);
    });
    sections->addWidget(body);
    versionTable = new QTableWidget(0, 3);
    versionTable->setObjectName(s("project-versions-table"));
    versionTable->setHorizontalHeaderLabels({tr("Версия"), tr("Minecraft"), tr("Загрузчик")});
    versionTable->horizontalHeader()->setSectionResizeMode(QHeaderView::Stretch);
    versionTable->verticalHeader()->hide();
    versionTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    versionTable->setSelectionMode(QAbstractItemView::SingleSelection);
    versionTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    versionTable->setShowGrid(false);
    sections->addWidget(versionTable);
    connect(tabs, &QTabBar::currentChanged, sections, &QStackedWidget::setCurrentIndex);
    connect(versionTable, &QTableWidget::cellClicked, this,
            [this](int row, int) { version->setCurrentIndex(row); });
    left->addWidget(sections, 1);
    columns->addLayout(left, 1);
    auto *installPanel = panel();
    installPanel->setMinimumHeight(430);
    auto *r = new QVBoxLayout(installPanel);
    r->setContentsMargins(18, 18, 18, 18);
    r->setSpacing(10);
    r->addWidget(label(tr("Установка"), "sectionTitle"));
    target = new QComboBox;
    target->setObjectName(s("project-target"));
    target->setToolTip(tr("Установить в сборку"));
    r->addWidget(target);
    r->addWidget(label(tr("Minecraft"), "mutedSmall"));
    game = new QComboBox;
    game->setObjectName(s("project-game"));
    r->addWidget(game);
    r->addWidget(label(tr("Загрузчик"), "mutedSmall"));
    loader = new QComboBox;
    loader->setObjectName(s("project-loader"));
    r->addWidget(loader);
    r->addWidget(label(tr("Версия проекта"), "mutedSmall"));
    version = new QComboBox;
    version->setObjectName(s("project-version"));
    version->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    version->setMinimumContentsLength(8);
    r->addWidget(version);
    for (auto *control : {target, game, loader, version})
        control->setFixedHeight(38);
    install = button(tr("Установить"), r, [this] { installSelected(); }, this, true);
    install->setFixedHeight(44);
    install->setObjectName(s("project-install"));
    status = label(QString(), "mutedSmall");
    status->setWordWrap(true);
    r->addWidget(status);
    auto *installScroll = new QScrollArea;
    installScroll->setObjectName(s("project-install-scroll"));
    installScroll->setFixedWidth(270);
    installScroll->setWidgetResizable(true);
    installScroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    installScroll->setWidget(installPanel);
    columns->addWidget(installScroll);
    layout->addLayout(columns, 1);
    connect(game, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this] { filterVersions(); });
    connect(loader, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this] { filterVersions(); });
    connect(target, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this] { targetChanged(); });
}

bool ProjectView::compatible(const QJsonObject &v, const QJsonObject &build, const QString &type) {
    if (type == s("modpack"))
        return true;
    return !build.isEmpty() &&
           v.value(s("game_versions")).toArray().contains(baseGameVersion(build)) &&
           (type != s("mod") || v.value(s("loaders")).toArray().contains(value(build, "loader")));
}

void ProjectView::open(const QJsonObject &input, const QJsonArray &list, const QString &selected) {
    const auto request = ++generation;
    project = input;
    const auto id =
        value(input, "project_id").isEmpty() ? value(input, "id") : value(input, "project_id");
    project.insert(s("id"), id);
    builds = list;
    allVersions = {};
    title->setText(value(input, "title"));
    description->setText(value(input, "description"));
    icon->setImage({});
    icon->setFallback(title->text());
    // A new document invalidates old image callbacks when another project is opened.
    auto *previous = body->document();
    body->setDocument(new ProjectDocument(images, body));
    if (previous->parent() == body)
        previous->deleteLater();
    body->setPlainText(tr("Загружаем описание…"));
    tabs->setCurrentIndex(0);
    {
        QSignalBlocker a(game), b(loader), c(target);
        game->clear();
        game->addItem(tr("Все версии"), QString());
        loader->clear();
        loader->addItem(tr("Все загрузчики"), QString());
        target->clear();
        for (const auto &entry : builds) {
            const auto build = entry.toObject();
            target->addItem(value(build, "name"), value(build, "id"));
        }
        target->setCurrentIndex(qMax(0, target->findData(selected)));
    }
    target->setVisible(value(project, "project_type") != s("modpack"));
    version->clear();
    versionTable->setRowCount(0);
    install->setEnabled(false);
    status->setText(tr("Загружаем версии…"));
    QPointer<ProjectView> guard(this);
    backend->request(
        s("modrinth_project"), {{s("projectId"), id}},
        [guard, request](const QJsonValue &v, const QJsonObject &error) {
            if (!guard || request != guard->generation)
                return;
            if (!error.isEmpty()) {
                guard->body->setPlainText(value(error, "message"));
                return;
            }
            const auto metadata = v.toObject();
            for (auto it = metadata.begin(); it != metadata.end(); ++it)
                guard->project.insert(it.key(), it.value());
            guard->title->setText(value(guard->project, "title"));
            guard->description->setText(value(guard->project, "description"));
            guard->body->setMarkdown(value(metadata, "body").left(2 * 1024 * 1024));
            // Markdown assigns its own link format; keep links legible on our dark page.
            QVector<QPair<int, int>> anchors;
            for (auto block = guard->body->document()->begin(); block.isValid();
                 block = block.next())
                for (auto fragment = block.begin(); !fragment.atEnd(); ++fragment) {
                    const auto span = fragment.fragment();
                    if (!span.isValid() || !span.charFormat().isAnchor())
                        continue;
                    anchors.append({span.position(), span.length()});
                }
            for (const auto &anchor : anchors) {
                QTextCursor cursor(guard->body->document());
                cursor.setPosition(anchor.first);
                cursor.setPosition(anchor.first + anchor.second, QTextCursor::KeepAnchor);
                QTextCharFormat format;
                format.setForeground(QColor(98, 212, 255));
                cursor.mergeCharFormat(format);
            }
            guard->images->load(value(guard->project, "icon_url"), guard,
                                [guard, request](const QImage &image) {
                                    if (guard && request == guard->generation)
                                        guard->icon->setImage(image);
                                });
            guard->target->setVisible(value(guard->project, "project_type") != s("modpack"));
            guard->filterVersions();
        });
    backend->request(
        s("modrinth_project_versions"), {{s("projectId"), id}},
        [guard, request](const QJsonValue &v, const QJsonObject &error) {
            if (!guard || request != guard->generation)
                return;
            if (!error.isEmpty()) {
                guard->status->setText(value(error, "message"));
                return;
            }
            guard->allVersions = v.toArray();
            QSet<QString> games, loaders;
            for (const auto &entry : guard->allVersions) {
                const auto version = entry.toObject();
                for (const auto &game : strings(version.value(s("game_versions")).toArray()))
                    games.insert(game);
                for (const auto &loader : strings(version.value(s("loaders")).toArray()))
                    loaders.insert(loader);
            }
            auto gameList = games.values(), loaderList = loaders.values();
            QCollator numeric;
            numeric.setNumericMode(true);
            std::sort(gameList.begin(), gameList.end(),
                      [&numeric](const QString &a, const QString &b) {
                          return numeric.compare(a, b) > 0;
                      });
            loaderList.sort();
            {
                QSignalBlocker a(guard->game), b(guard->loader);
                for (const auto &game : gameList)
                    guard->game->addItem(game, game);
                for (const auto &loader : loaderList)
                    guard->loader->addItem(loader, loader);
            }
            guard->targetChanged();
        });
}

void ProjectView::targetChanged() {
    QSignalBlocker a(game), b(loader);
    if (value(project, "project_type") != s("modpack")) {
        for (const auto &entry : builds) {
            const auto build = entry.toObject();
            if (value(build, "id") != target->currentData().toString())
                continue;
            game->setCurrentIndex(qMax(0, game->findData(baseGameVersion(build))));
            loader->setCurrentIndex(value(project, "project_type") == s("mod")
                                        ? qMax(0, loader->findData(value(build, "loader")))
                                        : 0);
        }
    }
    filterVersions();
}

void ProjectView::filterVersions() {
    const auto previous = version->currentData().toString();
    version->clear();
    versionTable->setRowCount(0);
    QJsonObject build;
    for (const auto &entry : builds)
        if (value(entry.toObject(), "id") == target->currentData().toString())
            build = entry.toObject();
    for (const auto &entry : allVersions) {
        const auto v = entry.toObject();
        const auto games = v.value(s("game_versions")).toArray(),
                   loaders = v.value(s("loaders")).toArray();
        if ((!game->currentData().toString().isEmpty() &&
             !games.contains(game->currentData().toString())) ||
            (!loader->currentData().toString().isEmpty() &&
             !loaders.contains(loader->currentData().toString())) ||
            !compatible(v, build, value(project, "project_type")))
            continue;
        const auto name = value(v, "version_number") + s(" · ") + value(v, "version_type");
        version->addItem(name, value(v, "id"));
        const auto row = versionTable->rowCount();
        versionTable->insertRow(row);
        cells(versionTable, row,
              {name, strings(games).join(s(", ")), strings(loaders).join(s(", "))});
        versionTable->setRowHeight(row, 52);
    }
    if (version->findData(previous) >= 0)
        version->setCurrentIndex(version->findData(previous));
    install->setEnabled(version->count() > 0);
    status->setText(version->count()
                        ? tr("%1 доступных версий. Устанавливайте проекты, которым доверяете.")
                              .arg(version->count())
                        : tr("Совместимых версий нет. Выберите другую версию Minecraft, загрузчик "
                             "или сборку."));
}

void ProjectView::installSelected() {
    if (version->currentIndex() < 0)
        return;
    const bool pack = value(project, "project_type") == s("modpack");
    install->setEnabled(false);
    status->setText(tr("Установка началась. Ход операции — справа сверху."));
    emit installRequested(value(project, "id"), version->currentData().toString(),
                          pack ? QString() : target->currentData().toString(), pack);
}
void ProjectView::setInstalling(bool active) {
    install->setEnabled(!active && version->count() > 0);
    if (!active)
        filterVersions();
}
