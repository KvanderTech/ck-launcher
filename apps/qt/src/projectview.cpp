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
    void fitImages(int availableWidth) {
        const int next = qMax(80, availableWidth - 44);
        if (next == imageWidth)
            return;
        imageWidth = next;
        for (auto it = originals.cbegin(); it != originals.cend(); ++it)
            addResource(QTextDocument::ImageResource, it.key(), fitted(it.value()));
        if (!originals.isEmpty())
            markContentsDirty(0, characterCount());
    }

  protected:
    QVariant loadResource(int type, const QUrl &url) override {
        QImage placeholder(1, 1, QImage::Format_ARGB32);
        placeholder.fill(Qt::transparent);
        if (type != QTextDocument::ImageResource || ImagePool::remoteUrl(url.toString()).isEmpty())
            return placeholder;
        if (!pending.contains(url)) {
            pending.insert(url);
            QTimer::singleShot(0, this, [this, url] {
                images->load(url.toString(), this, [this, url](const QImage &image) {
                    if (image.isNull())
                        return;
                    // Bound retained full-size images per document independently of the shared
                    // cache.
                    const auto source = image.width() > 1600 || image.height() > 1600
                                            ? image.scaled(QSize(1600, 1600), Qt::KeepAspectRatio,
                                                           Qt::SmoothTransformation)
                                            : image;
                    const auto cost = qint64(source.width()) * source.height() * 4;
                    if (originalBytes + cost > 32 * 1024 * 1024)
                        return;
                    originalBytes += cost;
                    originals.insert(url, source);
                    addResource(QTextDocument::ImageResource, url, fitted(source));
                    markContentsDirty(0, characterCount());
                });
            });
        }
        return placeholder;
    }

  private:
    ImagePool *images;
    QSet<QUrl> pending;
    QHash<QUrl, QImage> originals;
    qint64 originalBytes = 0;
    int imageWidth = 600;
    QImage fitted(const QImage &image) const {
        return image.scaledToWidth(qMin(image.width(), imageWidth), Qt::SmoothTransformation);
    }
};
class ProjectBrowser final : public QTextBrowser {
  protected:
    void resizeEvent(QResizeEvent *event) override {
        QTextBrowser::resizeEvent(event);
        if (auto *doc = dynamic_cast<ProjectDocument *>(document()))
            doc->fitImages(viewport()->width());
    }
};
QStringList strings(const QJsonArray &array) {
    QStringList result;
    for (const auto &item : array)
        if (!item.toString().isEmpty() && !result.contains(item.toString()))
            result << item.toString();
    return result;
}
QString loaderName(const QString &loader) {
    if (loader == s("fabric"))
        return s("Fabric");
    if (loader == s("quilt"))
        return s("Quilt");
    if (loader == s("forge"))
        return s("Forge");
    if (loader == s("neoforge"))
        return s("NeoForge");
    if (loader == s("minecraft") || loader == s("vanilla"))
        return s("Minecraft");
    return loader;
}
QString channelName(const QJsonObject &version) {
    const auto channel = value(version, "version_type");
    if (channel == s("release"))
        return ProjectView::tr("Релиз");
    if (channel == s("beta"))
        return ProjectView::tr("Бета");
    if (channel == s("alpha"))
        return ProjectView::tr("Альфа");
    return channel;
}
QString versionName(const QJsonObject &version) {
    auto name = value(version, "version_number");
    if (name.isEmpty())
        name = value(version, "name");
    return name.isEmpty() ? value(version, "id") : name;
}
enum VersionRole { VersionId = Qt::UserRole, VersionTitle, VersionSubtitle };

// Keep the version list readable even when Minecraft compatibility contains dozens of entries.
// Full text remains available in the tooltip and accessible item text; only its painting elides.
class VersionDelegate final : public QStyledItemDelegate {
  public:
    using QStyledItemDelegate::QStyledItemDelegate;
    void paint(QPainter *painter, const QStyleOptionViewItem &option,
               const QModelIndex &index) const override {
        if (index.column() != 0) {
            QStyledItemDelegate::paint(painter, option, index);
            return;
        }
        painter->save();
        if (option.state & QStyle::State_Selected)
            painter->fillRect(option.rect, QColor(18, 62, 87));
        else if (option.state & QStyle::State_MouseOver)
            painter->fillRect(option.rect, QColor(15, 44, 64));
        const auto rect = option.rect.adjusted(16, 12, -12, -10);
        auto font = option.font;
        font.setPixelSize(14);
        font.setWeight(QFont::DemiBold);
        painter->setFont(font);
        painter->setPen(QColor(240, 247, 252));
        painter->drawText(QRect(rect.left(), rect.top(), rect.width(), 22),
                          Qt::AlignLeft | Qt::AlignVCenter,
                          QFontMetrics(font).elidedText(index.data(VersionTitle).toString(),
                                                        Qt::ElideRight, rect.width()));
        font.setPixelSize(12);
        font.setWeight(QFont::Normal);
        painter->setFont(font);
        painter->setPen(QColor(147, 184, 207));
        painter->drawText(QRect(rect.left(), rect.top() + 26, rect.width(), 20),
                          Qt::AlignLeft | Qt::AlignVCenter,
                          QFontMetrics(font).elidedText(index.data(VersionSubtitle).toString(),
                                                        Qt::ElideRight, rect.width()));
        painter->setPen(QColor(44, 72, 92, 140));
        painter->drawLine(option.rect.bottomLeft() + QPoint(16, 0), option.rect.bottomRight());
        painter->restore();
    }
};
} // namespace

ProjectView::ProjectView(Backend *core, ImagePool *pool, QWidget *parent)
    : QWidget(parent), backend(core), images(pool) {
    setObjectName(s("project-page"));
    setMinimumWidth(0);
    auto *layout = new QVBoxLayout(this);
    // Switching from stacked filters back to columns must not leave the page's old
    // minimum height on the host window. The description/list and filters scroll themselves.
    layout->setSizeConstraint(QLayout::SetNoConstraint);
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
    site->setMinimumWidth(136);
    layout->addLayout(top);
    auto *head = panel(s("hero"));
    auto *h = new QHBoxLayout(head);
    h->setContentsMargins(20, 16, 20, 16);
    icon = new Picture(64);
    h->addWidget(icon);
    auto *copy = new QVBoxLayout;
    title = label(QString(), "sectionTitle");
    title->setWordWrap(true);
    title->setTextFormat(Qt::PlainText);
    title->setMinimumWidth(0);
    description = label(QString(), "muted");
    description->setWordWrap(true);
    description->setTextFormat(Qt::PlainText);
    description->setMinimumWidth(0);
    copy->addWidget(title);
    copy->addWidget(description);
    h->addLayout(copy, 1);
    layout->addWidget(head);
    columns = new QBoxLayout(QBoxLayout::LeftToRight);
    columns->setSpacing(16);
    leftColumn = new QWidget;
    leftColumn->setMinimumWidth(0);
    leftColumn->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
    auto *left = new QVBoxLayout(leftColumn);
    left->setContentsMargins(0, 0, 0, 0);
    left->setSpacing(12);
    tabs = new QTabBar;
    tabs->setObjectName(s("project-tabs"));
    tabs->setDrawBase(false);
    tabs->setExpanding(false);
    tabs->addTab(tr("Описание"));
    tabs->addTab(tr("Версии"));
    left->addWidget(tabs, 0, Qt::AlignLeft);
    auto *sections = new QStackedWidget;
    sections->setMinimumSize(0, 150);
    body = new ProjectBrowser;
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
    auto *versionsSection = new QWidget;
    versionsSection->setMinimumWidth(0);
    auto *versionsLayout = new QVBoxLayout(versionsSection);
    versionsLayout->setContentsMargins(0, 0, 0, 0);
    versionsLayout->setSpacing(8);
    versionCount = label(QString(), "muted");
    versionCount->setObjectName(s("project-versions-count"));
    versionsLayout->addWidget(versionCount);
    versionPages = new QStackedWidget;
    versionTable = new QTableWidget(0, 2);
    versionTable->setObjectName(s("project-versions-table"));
    versionTable->setHorizontalHeaderLabels({tr("Версия и совместимость"), tr("Установка")});
    versionTable->setMinimumWidth(0);
    versionTable->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Expanding);
    versionTable->setItemDelegate(new VersionDelegate(versionTable));
    versionTable->horizontalHeader()->setSectionResizeMode(0, QHeaderView::Stretch);
    versionTable->horizontalHeader()->setSectionResizeMode(1, QHeaderView::Fixed);
    versionTable->setColumnWidth(1, 136);
    versionTable->horizontalHeader()->hide();
    versionTable->verticalHeader()->hide();
    versionTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    versionTable->setSelectionMode(QAbstractItemView::SingleSelection);
    versionTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    versionTable->setShowGrid(false);
    versionTable->setMouseTracking(true);
    versionTable->setFocusPolicy(Qt::StrongFocus);
    versionTable->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    versionTable->setVerticalScrollMode(QAbstractItemView::ScrollPerPixel);
    versionTable->setStyleSheet(s("QTableWidget {background:rgba(8,26,44,240);"
                                  "border:1px solid #27415a;border-radius:16px;padding:6px;}"));
    versionPages->addWidget(versionTable);
    auto *empty = panel();
    empty->setObjectName(s("project-versions-empty"));
    auto *emptyLayout = new QVBoxLayout(empty);
    emptyLayout->setContentsMargins(22, 22, 22, 22);
    emptyLayout->setSpacing(10);
    emptyLayout->addStretch();
    emptyTitle = label(QString(), "strong");
    emptyTitle->setAlignment(Qt::AlignCenter);
    emptyTitle->setWordWrap(true);
    emptyText = label(QString(), "muted");
    emptyText->setTextFormat(Qt::PlainText);
    emptyText->setAlignment(Qt::AlignCenter);
    emptyText->setWordWrap(true);
    emptyLayout->addWidget(emptyTitle);
    emptyLayout->addWidget(emptyText);
    retry = button(
        tr("Повторить загрузку"), emptyLayout,
        [this] { open(project, builds, target->currentData().toString()); }, this);
    retry->setObjectName(s("project-retry"));
    emptyLayout->setAlignment(retry, Qt::AlignHCenter);
    emptyLayout->addStretch();
    versionPages->addWidget(empty);
    versionsLayout->addWidget(versionPages, 1);
    sections->addWidget(versionsSection);
    connect(tabs, &QTabBar::currentChanged, sections, &QStackedWidget::setCurrentIndex);
    connect(versionTable, &QTableWidget::currentCellChanged, this, [this](int row, int, int, int) {
        if (const auto *item = versionTable->item(row, 0))
            selectVersion(item->data(VersionId).toString());
    });
    left->addWidget(sections, 1);
    columns->addWidget(leftColumn, 1);
    installPanel = panel();
    installPanel->setMinimumWidth(0);
    installPanel->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    installLayout = new QGridLayout(installPanel);
    installLayout->setContentsMargins(18, 18, 18, 18);
    installLayout->setHorizontalSpacing(12);
    installLayout->setVerticalSpacing(12);
    installTitle = label(tr("Установка"), "sectionTitle");
    target = new QComboBox;
    target->setObjectName(s("project-target"));
    target->setToolTip(tr("Установить в сборку"));
    game = new QComboBox;
    game->setObjectName(s("project-game"));
    loader = new QComboBox;
    loader->setObjectName(s("project-loader"));
    version = new QComboBox;
    version->setObjectName(s("project-version"));
    for (auto *control : {target, game, loader, version}) {
        control->setFixedHeight(38);
        control->setMinimumWidth(0);
        control->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
        control->setMinimumContentsLength(5);
        control->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    }
    const auto field = [](const QString &name, QComboBox *control) {
        auto *container = new QWidget;
        container->setMinimumWidth(0);
        auto *layout = new QVBoxLayout(container);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->setSpacing(6);
        auto *caption = label(name, "mutedSmall");
        caption->setBuddy(control);
        layout->addWidget(caption);
        layout->addWidget(control);
        return container;
    };
    targetField = field(tr("В сборку"), target);
    gameField = field(tr("Minecraft"), game);
    loaderField = field(tr("Загрузчик"), loader);
    versionField = field(tr("Версия проекта"), version);
    install = new MotionButton(tr("Установить"));
    install->setProperty("primary", true);
    connect(install, &QPushButton::clicked, this, [this] { installSelected(); });
    install->setFixedHeight(44);
    install->setObjectName(s("project-install"));
    status = label(QString(), "mutedSmall");
    status->setObjectName(s("project-status"));
    status->setTextFormat(Qt::PlainText);
    status->setWordWrap(true);
    status->setMinimumWidth(0);
    installScroll = new QScrollArea;
    installScroll->setObjectName(s("project-install-scroll"));
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
    connect(version, qOverload<int>(&QComboBox::currentIndexChanged), this,
            [this] { selectVersion(version->currentData().toString()); });
    arrangeColumns();
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
    projectLoading = versionsLoading = true;
    projectError.clear();
    versionsError.clear();
    installationError.clear();
    title->setText(value(input, "title"));
    description->setText(value(input, "description"));
    icon->setImage({});
    icon->setFallback(title->text());
    // A new document invalidates old image callbacks when another project is opened.
    // QTextBrowser can synchronously delete its default document in setDocument().
    // Our own documents belong to body and need deferred cleanup when replaced.
    QPointer<QTextDocument> previous = body->document();
    body->setDocument(new ProjectDocument(images, body));
    if (previous && previous->parent() == body)
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
    targetField->setVisible(value(project, "project_type") != s("modpack"));
    {
        QSignalBlocker a(version), b(versionTable);
        version->clear();
        versionTable->setRowCount(0);
    }
    filterVersions();
    QPointer<ProjectView> guard(this);
    backend->request(
        s("modrinth_project"), {{s("projectId"), id}},
        [guard, request](const QJsonValue &v, const QJsonObject &error) {
            if (!guard || request != guard->generation)
                return;
            guard->projectLoading = false;
            if (!error.isEmpty()) {
                guard->projectError = value(error, "message");
                if (guard->projectError.isEmpty())
                    guard->projectError = tr("Не удалось загрузить описание проекта.");
                guard->body->setPlainText(guard->projectError);
                guard->filterVersions();
                return;
            }
            const auto metadata = v.toObject();
            for (auto it = metadata.begin(); it != metadata.end(); ++it)
                guard->project.insert(it.key(), it.value());
            guard->title->setText(value(guard->project, "title"));
            guard->description->setText(value(guard->project, "description"));
            if (metadata.contains(s("bodyHtml")))
                guard->body->setHtml(value(metadata, "bodyHtml"));
            else
                guard->body->setMarkdown(value(metadata, "body").left(2 * 1024 * 1024));
            if (auto *doc = dynamic_cast<ProjectDocument *>(guard->body->document()))
                doc->fitImages(guard->body->viewport()->width());
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
            guard->targetField->setVisible(value(guard->project, "project_type") != s("modpack"));
            guard->targetChanged();
        });
    backend->request(
        s("modrinth_project_versions"), {{s("projectId"), id}},
        [guard, request](const QJsonValue &v, const QJsonObject &error) {
            if (!guard || request != guard->generation)
                return;
            guard->versionsLoading = false;
            if (!error.isEmpty()) {
                guard->versionsError = value(error, "message");
                if (guard->versionsError.isEmpty())
                    guard->versionsError = tr("Не удалось загрузить версии проекта.");
                guard->filterVersions();
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
                    if (guard->game->findData(game) < 0)
                        guard->game->addItem(game, game);
                for (const auto &loader : loaderList)
                    if (guard->loader->findData(loader) < 0)
                        guard->loader->addItem(loaderName(loader), loader);
            }
            guard->targetChanged();
        });
}

void ProjectView::targetChanged() {
    QSignalBlocker a(game), b(loader);
    if (value(project, "project_type") != s("modpack")) {
        const auto build = selectedBuild();
        if (!build.isEmpty()) {
            const auto gameVersion = baseGameVersion(build), buildLoader = value(build, "loader");
            // An incompatible target should still display its actual Minecraft/loader,
            // instead of silently falling back to the misleading "All" filter.
            if (game->findData(gameVersion) < 0)
                game->addItem(gameVersion, gameVersion);
            if (loader->findData(buildLoader) < 0)
                loader->addItem(loaderName(buildLoader), buildLoader);
            game->setCurrentIndex(game->findData(gameVersion));
            loader->setCurrentIndex(
                value(project, "project_type") == s("mod") ? loader->findData(buildLoader) : 0);
        }
    }
    installationError.clear();
    filterVersions();
}

void ProjectView::filterVersions() {
    const auto previous = version->currentData().toString();
    const auto build = selectedBuild();
    {
        QSignalBlocker a(version), b(versionTable);
        version->clear();
        versionTable->setRowCount(0);
        QSet<QString> seen;
        for (const auto &entry : allVersions) {
            const auto v = entry.toObject();
            const auto id = value(v, "id");
            const auto games = v.value(s("game_versions")).toArray(),
                       loaders = v.value(s("loaders")).toArray();
            if (id.isEmpty() || seen.contains(id) ||
                (!game->currentData().toString().isEmpty() &&
                 !games.contains(game->currentData().toString())) ||
                (!loader->currentData().toString().isEmpty() &&
                 !loaders.contains(loader->currentData().toString())) ||
                !compatible(v, build, value(project, "project_type")))
                continue;
            seen.insert(id);
            const auto channel = channelName(v);
            const auto name = versionName(v) + (channel.isEmpty() ? QString() : s(" · ") + channel);
            version->addItem(name, id);
            QStringList loaderNames;
            for (const auto &loaderId : strings(loaders))
                loaderNames << loaderName(loaderId);
            QStringList details;
            if (!games.isEmpty())
                details << s("Minecraft ") + strings(games).join(s(", "));
            if (!loaderNames.isEmpty())
                details << loaderNames.join(s(", "));
            const auto published = QDateTime::fromString(value(v, "date_published"), Qt::ISODate);
            if (published.isValid())
                details << published.toLocalTime().date().toString(s("dd.MM.yyyy"));
            const auto subtitle = details.join(s(" · "));
            const auto row = versionTable->rowCount();
            versionTable->insertRow(row);
            auto *item = new QTableWidgetItem(name + s("\n") + subtitle);
            item->setData(VersionId, id);
            item->setData(VersionTitle, name);
            item->setData(VersionSubtitle, subtitle);
            item->setToolTip(name + s("\n") + subtitle);
            versionTable->setItem(row, 0, item);
            auto *actionCell = new QWidget;
            auto *rowLayout = new QHBoxLayout(actionCell);
            rowLayout->setContentsMargins(4, 16, 12, 16);
            auto *action = button(
                tr("Установить"), rowLayout,
                [this, id] {
                    if (installing)
                        return;
                    selectVersion(id);
                    if (version->currentData().toString() == id)
                        installSelected();
                },
                this, true);
            action->setObjectName(s("project-version-install-") + id);
            action->setProperty("versionId", id);
            action->setAccessibleName(tr("Установить версию %1").arg(name));
            action->setToolTip(tr("Установить именно эту версию: %1").arg(name));
            action->setFixedHeight(36);
            action->setStyleSheet(
                s("QPushButton {font-size:12px;padding:6px 10px;border-radius:10px;}"));
            versionTable->setCellWidget(row, 1, actionCell);
            versionTable->setRowHeight(row, 74);
        }
        const auto restored = version->findData(previous);
        version->setCurrentIndex(restored >= 0 ? restored : (version->count() ? 0 : -1));
    }
    selectVersion(version->currentData().toString());
    updateActions();
}

void ProjectView::installSelected() {
    if (installing || !actionBlockedReason.isEmpty() || projectLoading || versionsLoading ||
        !projectError.isEmpty() || !versionsError.isEmpty() || version->currentIndex() < 0)
        return;
    const bool pack = value(project, "project_type") == s("modpack");
    const auto id = version->currentData().toString();
    const auto build = selectedBuild();
    bool allowed = false;
    for (const auto &entry : allVersions)
        if (value(entry.toObject(), "id") == id &&
            compatible(entry.toObject(), build, value(project, "project_type"))) {
            allowed = true;
            break;
        }
    if (!allowed || value(project, "id").isEmpty() || (!pack && build.isEmpty()))
        return;
    setInstalling(true);
    emit installRequested(value(project, "id"), id,
                          pack ? QString() : target->currentData().toString(), pack);
}
void ProjectView::setInstalling(bool active) {
    if (installing == active)
        return;
    installing = active;
    installationError.clear();
    updateActions();
}

void ProjectView::setInstallationError(const QString &message) {
    installing = false;
    installationError =
        message.isEmpty() ? tr("Не удалось установить проект. Повторите попытку.") : message;
    updateActions();
}

void ProjectView::setActionBlockedReason(const QString &reason) {
    if (actionBlockedReason == reason)
        return;
    actionBlockedReason = reason;
    updateActions();
}

QJsonObject ProjectView::selectedBuild() const {
    for (const auto &entry : builds)
        if (value(entry.toObject(), "id") == target->currentData().toString())
            return entry.toObject();
    return {};
}

void ProjectView::selectVersion(const QString &id) {
    const auto index = version->findData(id);
    QSignalBlocker a(version), b(versionTable);
    version->setCurrentIndex(index);
    for (int row = 0; row < versionTable->rowCount(); ++row)
        if (versionTable->item(row, 0)->data(VersionId).toString() == id) {
            versionTable->setCurrentCell(row, 0);
            return;
        }
    versionTable->clearSelection();
    versionTable->setCurrentCell(-1, -1);
}

void ProjectView::updateActions() {
    const bool loading = projectLoading || versionsLoading;
    const auto loadError = !versionsError.isEmpty() ? versionsError : projectError;
    const bool ready = !loading && loadError.isEmpty();
    const bool canInstall =
        ready && !installing && actionBlockedReason.isEmpty() && version->currentIndex() >= 0 &&
        (value(project, "project_type") == s("modpack") || !selectedBuild().isEmpty());
    install->setEnabled(canInstall);
    install->setText(installing && actionBlockedReason.isEmpty() ? tr("Устанавливаем…")
                                                                 : tr("Установить"));
    for (auto *control : {target, game, loader, version})
        control->setEnabled(ready && !installing && control->count() > 0);
    versionTable->setEnabled(!installing);
    for (auto *action : versionTable->findChildren<QPushButton *>())
        if (action->property("versionId").isValid())
            action->setEnabled(canInstall);
    retry->setVisible(!loading && !loadError.isEmpty());
    retry->setEnabled(!installing);
    versionCount->setText(loading                ? tr("Загружаем версии…")
                          : !loadError.isEmpty() ? tr("Версии недоступны")
                                                 : tr("Доступно версий: %1").arg(version->count()));
    if (loading) {
        emptyTitle->setText(tr("Загружаем версии"));
        emptyText->setText(tr("Получаем данные проекта с Modrinth…"));
    } else if (!loadError.isEmpty()) {
        emptyTitle->setText(tr("Не удалось загрузить проект"));
        emptyText->setText(loadError);
    } else if (value(project, "project_type") != s("modpack") && target->count() == 0) {
        emptyTitle->setText(tr("Сначала добавьте сборку"));
        emptyText->setText(tr("Моды, ресурспаки и шейдеры устанавливаются в выбранную сборку. "
                              "Создайте или установите её в библиотеке."));
    } else {
        emptyTitle->setText(tr("Нет совместимых версий"));
        emptyText->setText(
            tr("Измените фильтры Minecraft и загрузчика или выберите другую сборку."));
    }
    versionPages->setCurrentIndex(ready && version->count() > 0 ? 0 : 1);
    if (!actionBlockedReason.isEmpty())
        status->setText(actionBlockedReason);
    else if (installing)
        status->setText(tr("Установка выполняется. Прогресс и отмена — справа сверху."));
    else if (!installationError.isEmpty())
        status->setText(installationError);
    else if (!loadError.isEmpty())
        status->setText(loadError);
    else if (loading)
        status->setText(tr("Загружаем данные проекта…"));
    else if (version->count())
        status->setText(tr("Выберите версию здесь или установите её прямо из списка «Версии»."));
    else
        status->setText(emptyText->text());
    status->setStyleSheet(actionBlockedReason.isEmpty() &&
                                  (!installationError.isEmpty() || !loadError.isEmpty())
                              ? s("color:#ef96b0;")
                              : QString());
    arrangeColumns();
}

void ProjectView::resizeEvent(QResizeEvent *event) {
    QWidget::resizeEvent(event);
    arrangeColumns();
}

void ProjectView::arrangeColumns() {
    const bool compact = width() < 840;
    if (installLayout->count() == 0 || narrow != compact) {
        narrow = compact;
        while (auto *item = installLayout->takeAt(0))
            delete item;
        for (int row = 0; row < 8; ++row)
            installLayout->setRowStretch(row, 0);
        installLayout->addWidget(installTitle, 0, 0, 1, 2);
        installLayout->addWidget(targetField, 1, 0, 1, 2);
        installLayout->addWidget(gameField, 2, 0, 1, narrow ? 1 : 2);
        installLayout->addWidget(loaderField, narrow ? 2 : 3, narrow ? 1 : 0, 1, narrow ? 1 : 2);
        installLayout->addWidget(versionField, narrow ? 3 : 4, 0, 1, narrow ? 1 : 2);
        installLayout->addWidget(install, narrow ? 3 : 5, narrow ? 1 : 0, 1, narrow ? 1 : 2,
                                 Qt::AlignBottom);
        installLayout->addWidget(status, narrow ? 4 : 6, 0, 1, 2);
        if (!narrow)
            installLayout->setRowStretch(7, 1);
        columns->removeWidget(leftColumn);
        columns->removeWidget(installScroll);
        columns->setDirection(narrow ? QBoxLayout::TopToBottom : QBoxLayout::LeftToRight);
        columns->addWidget(narrow ? static_cast<QWidget *>(installScroll) : leftColumn,
                           narrow ? 0 : 1);
        columns->addWidget(narrow ? leftColumn : static_cast<QWidget *>(installScroll),
                           narrow ? 1 : 0);
        installScroll->setMinimumSize(narrow ? QSize(0, 0) : QSize(270, 0));
        installScroll->setMaximumSize(narrow ? QSize(QWIDGETSIZE_MAX, 300)
                                             : QSize(270, QWIDGETSIZE_MAX));
    }
    if (narrow)
        installScroll->setFixedHeight(qBound(180, installPanel->sizeHint().height(), 300));
}
