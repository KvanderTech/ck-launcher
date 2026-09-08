#include "window.h"
#include <QSettings>
#ifdef Q_OS_WIN
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#endif
namespace {
class ShortcutScroll final : public QScrollArea {
  public:
    QSize sizeHint() const override {
        return QSize(48, qMin(278, maximumHeight()));
    }
    QSize minimumSizeHint() const override {
        return QSize(0, 0);
    }
};
class TitleBar final : public QWidget {
  public:
    explicit TitleBar(QWidget *parent) : QWidget(parent) {
        setFixedHeight(64);
    }

  protected:
    void mousePressEvent(QMouseEvent *e) override {
        if (e->button() == Qt::LeftButton && window()->windowHandle())
            window()->windowHandle()->startSystemMove();
    }
    void mouseDoubleClickEvent(QMouseEvent *e) override {
        if (e->button() == Qt::LeftButton) {
            if (window()->isMaximized())
                window()->showNormal();
            else
                window()->showMaximized();
        }
    }
};
class HomeArtwork final : public QWidget {
  public:
    HomeArtwork() : image(s(":/assets/home-render.png")) {
        setObjectName(s("home-artwork"));
        setAccessibleName(tr("Лаунчер для комфортной игры"));
        setMinimumHeight(350);
    }

  protected:
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        p.setRenderHint(QPainter::SmoothPixmapTransform);
        const qreal x = width() * .055;
        QFont captionFont(s("Segoe UI"));
        captionFont.setPixelSize(30);
        captionFont.setWeight(QFont::Bold);
        p.setFont(captionFont);
        p.setPen(QColor(195, 215, 231));
        p.drawText(QRectF(x, 24, width() * .46, 45), Qt::AlignLeft | Qt::AlignTop,
                   tr("Лаунчер для"));
        QFont large(s("Segoe UI"));
        large.setPixelSize(width() < 900 ? 42 : 48);
        large.setWeight(QFont::Black);
        large.setLetterSpacing(QFont::PercentageSpacing, 96);
        p.setFont(large);
        QLinearGradient color(x, 0, x + 390, 0);
        color.setColorAt(0, QColor(249, 253, 255));
        color.setColorAt(1, QColor(80, 197, 246));
        p.setPen(QPen(QBrush(color), 1));
        p.drawText(QRectF(x, 54, width() * .50, 70), Qt::AlignLeft | Qt::AlignTop,
                   tr("комфортной"));
        p.drawText(QRectF(x, 104, width() * .50, 70), Qt::AlignLeft | Qt::AlignTop, tr("игры"));
        QSizeF size = image.size();
        size.scale(QSizeF(width() * .50, height() * .75), Qt::KeepAspectRatio);
        p.drawPixmap(QRectF(width() - size.width() - 8, height() * .58 - size.height() / 2,
                            size.width(), size.height()),
                     image, image.rect());
    }

  private:
    QPixmap image;
};
} // namespace
void LauncherWindow::bringToFront() {
    if (isMinimized())
        setWindowState(windowState() & ~Qt::WindowMinimized);
    show();
    raise();
    activateWindow();
#ifdef Q_OS_WIN
    SetForegroundWindow(reinterpret_cast<HWND>(winId()));
#endif
}
LauncherWindow::LauncherWindow(Backend *backend, QWidget *parent)
    : QMainWindow(parent), core(backend), images(new ImagePool(backend, this)) {
    setWindowTitle(tr("ЦК Лаунчер"));
    setWindowIcon(QIcon(s(":/assets/logo.png")));
    setWindowFlags(Qt::Window | Qt::FramelessWindowHint);
    resize(1280, 720);
    setMinimumSize(980, 620);
    background = new Backdrop;
    setCentralWidget(background);
    auto *body = new QHBoxLayout(background);
    body->setContentsMargins(0, 0, 0, 0);
    body->setSpacing(0);
    auto *side = new QWidget;
    side->setObjectName(s("sidebar"));
    side->setFixedWidth(72);
    auto *bar = new QVBoxLayout(side);
    bar->setContentsMargins(10, 18, 10, 12);
    bar->setSpacing(7);
    auto *brand = new QLabel;
    brand->setPixmap(QPixmap(s(":/assets/logo.png"))
                         .scaled(34, 34, Qt::KeepAspectRatio, Qt::SmoothTransformation));
    brand->setFixedHeight(40);
    brand->setAlignment(Qt::AlignCenter);
    bar->addWidget(brand);
    bar->addSpacing(13);
    const QStringList names{s("home"), s("library"), s("grid"), s("shirt"), s("settings")};
    const QStringList titles{tr("Главная"), tr("Библиотека"), tr("Каталог"), tr("Скины и плащи"),
                             tr("Настройки")};
    for (int i = 0; i < 5; ++i) {
        auto *b = iconButton(names[i], titles[i]);
        b->setObjectName(s("nav-") + names[i]);
        b->setCheckable(true);
        auto *glow = new QGraphicsDropShadowEffect(b);
        glow->setBlurRadius(15);
        glow->setOffset(0, 0);
        glow->setColor(QColor(36, 177, 255, 175));
        glow->setEnabled(false);
        b->setGraphicsEffect(glow);
        navigation.append(b);
        connect(b, &QPushButton::clicked, this, [this, i] { navigate(i); });
        if (i < 4)
            bar->addWidget(b, 0, Qt::AlignHCenter);
    }
    auto *divider = new QFrame;
    divider->setObjectName(s("divider"));
    divider->setFixedSize(34, 1);
    bar->addSpacing(6);
    bar->addWidget(divider, 0, Qt::AlignHCenter);
    bar->addSpacing(6);
    auto *buildList = new QWidget;
    sidebarBuilds = new QVBoxLayout(buildList);
    sidebarBuilds->setContentsMargins(0, 0, 0, 0);
    sidebarBuilds->setSpacing(8);
    auto *buildScroll = new ShortcutScroll;
    buildScroll->setObjectName(s("sidebar-builds-scroll"));
    buildScroll->setWidgetResizable(true);
    buildScroll->setWidget(buildList);
    buildScroll->setFrameShape(QFrame::NoFrame);
    buildScroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    buildScroll->setMaximumHeight(278);
    bar->addWidget(buildScroll);
    auto *add = iconButton(s("plus"), tr("Добавить сборку"));
    connect(add, &QPushButton::clicked, this, [this] { navigate(2); });
    bar->addWidget(add, 0, Qt::AlignHCenter);
    bar->addStretch(1);
    bar->addWidget(navigation[4], 0, Qt::AlignHCenter);
    bar->addSpacing(9);
    accountButton = iconButton(s("person"), tr("Аккаунты Minecraft"));
    accountButton->setObjectName(s("accountButton"));
    accountButton->setIconSize(QSize(38, 38));
    connect(accountButton, &QPushButton::clicked, this, &LauncherWindow::accountMenu);
    bar->addWidget(accountButton, 0, Qt::AlignHCenter);
    body->addWidget(side);
    auto *main = new QWidget;
    auto *mainLayout = new QVBoxLayout(main);
    mainLayout->setContentsMargins(0, 0, 0, 0);
    mainLayout->setSpacing(0);
    auto *chrome = new TitleBar(this);
    auto *controls = new QHBoxLayout(chrome);
    controls->setContentsMargins(0, 18, 24, 0);
    controls->setSpacing(0);
    controls->addStretch();
    for (const auto &name : {s("minus"), s("maximize"), s("close")}) {
        auto *b = iconButton(name, name == s("minus")      ? tr("Свернуть")
                                   : name == s("maximize") ? tr("Развернуть / восстановить")
                                                           : tr("Закрыть"));
        b->setIconSize(QSize(19, 19));
        b->setObjectName(s("window-") + name);
        controls->addWidget(b);
        connect(b, &QPushButton::clicked, this, [this, name] {
            if (name == s("minus"))
                showMinimized();
            else if (name == s("maximize")) {
                isMaximized() ? showNormal() : showMaximized();
            } else
                close();
        });
    }
    mainLayout->addWidget(chrome);
    pages = new QStackedWidget;
    auto *fade = new QVariantAnimation(pages);
    fade->setDuration(190);
    fade->setEasingCurve(QEasingCurve::OutCubic);
    auto *opacity = new QGraphicsOpacityEffect(pages);
    opacity->setOpacity(1);
    opacity->setEnabled(false);
    pages->setGraphicsEffect(opacity);
    connect(fade, &QVariantAnimation::valueChanged, pages,
            [opacity](const QVariant &v) { opacity->setOpacity(v.toReal()); });
    connect(fade, &QVariantAnimation::finished, pages, [opacity] { opacity->setEnabled(false); });
    connect(pages, &QStackedWidget::currentChanged, pages, [this, fade, opacity] {
        fade->stop();
        if (!isVisible() || qApp->property("reduceMotion").toBool()) {
            opacity->setEnabled(false);
            return;
        }
        opacity->setEnabled(true);
        fade->setStartValue(.55);
        fade->setEndValue(1.0);
        fade->start();
    });
    mainLayout->addWidget(pages, 1);
    body->addWidget(main, 1);
    pages->addWidget(homePage());
    pages->addWidget(libraryPage());
    pages->addWidget(catalogPage());
    pages->addWidget(accountsPage());
    pages->addWidget(settingsPage());
    pages->addWidget(detailPage());
    projectView = new ProjectView(core, images);
    pages->addWidget(projectView);
    connect(projectView, &ProjectView::back, this, [this] { navigate(projectReturnPage); });
    connect(
        projectView, &ProjectView::installRequested, this,
        [this](const QString &project, const QString &version, const QString &build, bool pack) {
            QJsonObject params{{s("projectId"), project}, {s("versionId"), version}};
            if (!pack)
                params.insert(s("buildId"), build);
            call(
                pack ? s("install_modrinth_modpack") : s("install_modrinth_project"), params,
                [this](const QJsonValue &) {
                    refreshLibrary();
                    navigate(1);
                },
                true);
        });
    activity = panel(s("activity"));
    activity->setParent(background);
    activity->setFixedWidth(345);
    auto *a = new QVBoxLayout(activity);
    a->setContentsMargins(16, 12, 16, 12);
    auto *top = new QHBoxLayout;
    activityTitle = label(tr("Подготовка игры"), "strong");
    top->addWidget(activityTitle, 1);
    stop = iconButton(s("close"), tr("Отменить / остановить"));
    stop->setFixedSize(26, 26);
    stop->setIconSize(QSize(16, 16));
    top->addWidget(stop);
    a->addLayout(top);
    status = label(QString(), "muted");
    status->setWordWrap(true);
    a->addWidget(status);
    progress = new QProgressBar;
    progress->setFixedHeight(4);
    progress->setTextVisible(false);
    a->addWidget(progress);
    connect(stop, &QPushButton::clicked, this, [this] {
        if (busy || running || !operationId.isEmpty())
            cancel();
        else
            activity->hide();
    });
    activity->hide();
    connect(core, &Backend::ready, this, &LauncherWindow::initialize);
    connect(core, &Backend::disconnected, this, [this](const QString &reason) {
        if (!closing) {
            ready = false;
            busy = false;
            message(reason, true);
            updatePlayState();
        }
    });
    connect(core, &Backend::event, this, [this](const QString &event, const QJsonValue &payload) {
        auto data = payload.toObject();
        const auto eventId = value(data, "operationId");
        if (!eventId.isEmpty() && (completedOperations.contains(eventId) ||
                                   (!operationId.isEmpty() && eventId != operationId)))
            return;
        auto complete = [this, eventId] {
            if (!eventId.isEmpty()) {
                if (completedOperations.size() >= 64)
                    completedOperations.erase(completedOperations.begin());
                completedOperations.insert(eventId);
            }
            running = false;
            operationId.clear();
        };
        if (event == s("launcher://progress")) {
            const double total = data.value(s("totalBytes")).toDouble(),
                         done = data.value(s("completedBytes")).toDouble();
            progress->setRange(0, total > 0 ? 100 : 0);
            if (total > 0)
                progress->setValue(qBound(0, int(done * 100 / total), 100));
            QString stage = value(data, "stage");
            static const QMap<QString, QString> stages{
                {s("resolving-metadata"), tr("Получаем информацию о версии")},
                {s("downloading"), tr("Скачиваем файлы")},
                {s("resolving-java"), tr("Подбираем Java")},
                {s("checking"), tr("Подготавливаем библиотеки")},
                {s("installing"), tr("Распаковываем файлы")},
                {s("launching"), tr("Запускаем Minecraft")}};
            message(stages.value(stage, tr("Подготавливаем Minecraft…")));
        } else if (event == s("launcher://game-started")) {
            AudioFeedback::play(s("game-ready"));
            running = true;
            operationId = value(data, "operationId");
            progress->setRange(0, 100);
            progress->setValue(100);
            message(tr("Minecraft запущен"));
            updatePlayState();
        } else if (event == s("launcher://game-exited")) {
            AudioFeedback::play(s("game-exit"));
            complete();
            message(tr("Игра завершена"));
            updatePlayState();
        } else if (event == s("launcher://error")) {
            if (data.value(s("terminal")).toBool(true))
                complete();
            message(value(data.value(s("error")).toObject(), "message"), true);
            updatePlayState();
        }
    });
    QSettings settings;
    qApp->setProperty("reduceMotion", !settings.value(s("motion"), true).toBool());
    background->setMotion(settings.value(s("motion"), true).toBool());
    if (settings.contains(s("geometry")))
        restoreGeometry(settings.value(s("geometry")).toByteArray());
    navigate(0);
}
QWidget *LauncherWindow::homePage() {
    auto *page = new QWidget;
    page->setObjectName(s("home-page"));
    auto *layout = new QVBoxLayout(page);
    layout->setContentsMargins(44, 7, 44, 32);
    auto *art = new HomeArtwork;
    layout->addWidget(art, 1);
    auto *socials = new QWidget(art);
    auto *links = new QHBoxLayout(socials);
    links->setContentsMargins(0, 0, 0, 0);
    links->setSpacing(7);
    const QStringList names{s("telegram"), s("discord"), s("github")},
        urls{s("https://t.me/comfortcentr"), s("https://discord.gg/2CkZsVN8nm"),
             s("https://github.com/KvanderTech/ck-launcher")};
    for (int i = 0; i < 3; ++i) {
        auto *b = iconButton(names[i], names[i]);
        b->setFixedSize(30, 30);
        b->setIconSize(QSize(22, 22));
        links->addWidget(b);
        connect(b, &QPushButton::clicked, this,
                [this, url = urls[i]] { call(s("open_external_url"), {{s("url"), url}}); });
    }
    socials->move(60, 174);
    socials->adjustSize();
    auto *dock = new QHBoxLayout;
    dock->addStretch();
    play = button(tr("Играть"), dock, [this] { launch(); }, this, true);
    play->setObjectName(s("playButton"));
    play->setProperty("playAction", true);
    play->setFixedSize(205, 56);
    layout->addLayout(dock);
    return page;
}
void LauncherWindow::navigate(int page) {
    currentPage = page;
    pages->setCurrentIndex(page == 6 ? 5 : page == 7 ? 6 : page);
    if (page == 6) {
        if (auto *tabs = findChild<QTabBar *>(s("buildTabs")))
            tabs->setCurrentIndex(3);
    }
    const QStringList names{s("home"), s("library"), s("grid"), s("shirt"), s("settings")};
    for (int i = 0; i < navigation.size(); ++i) {
        bool active =
            i == page || (page == 5 && i == 1) || (page == 6 && i == 1) || (page == 7 && i == 2);
        navigation[i]->setChecked(active);
        navigation[i]->graphicsEffect()->setEnabled(active);
        navigation[i]->setIcon(
            glyph(names[i], active ? QColor(88, 204, 255) : QColor(174, 199, 217)));
    }
    if (page == 2 && catalog.isEmpty() && ready)
        searchCatalog();
    if (page == 3 && ready)
        refreshSkins();
    if (page == 6 && ready)
        showLogs();
}
void LauncherWindow::showPage(const QString &name) {
    const QStringList names{s("home"),     s("library"), s("catalog"), s("skins"),
                            s("settings"), s("details"), s("logs")};
    int i = names.indexOf(name);
    if (i >= 0)
        navigate(i);
}
void LauncherWindow::message(const QString &text, bool error) {
    if (text.isEmpty())
        return;
    status->setText(text);
    activityTitle->setText(error     ? tr("Не удалось выполнить действие")
                           : running ? tr("Minecraft")
                           : busy    ? tr("Выполняется операция")
                                     : tr("ЦК Лаунчер"));
    activity->setProperty("error", error);
    polish(activity);
    activity->adjustSize();
    activity->move(width() - activity->width() - 28, 65);
    activity->show();
    activity->raise();
    if (!busy && !running && operationId.isEmpty() && !error)
        QTimer::singleShot(4500, this, [this, text] {
            if (!busy && !running && operationId.isEmpty() && status->text() == text)
                activity->hide();
        });
}
void LauncherWindow::call(const QString &method, const QJsonObject &params,
                          std::function<void(const QJsonValue &)> done, bool mutation) {
    if (mutation && (busy || running || !operationId.isEmpty())) {
        const auto reason = tr("Дождитесь завершения текущей операции или остановите её.");
        if (method == s("install_update"))
            updateStatus->setText(reason);
        projectView->setInstallationError(reason);
        updatePlayState();
        message(reason, true);
        return;
    }
    const bool quiet = method == s("select_build");
    if (mutation) {
        busy = true;
        contentInstalling =
            method == s("install_modrinth_project") || method == s("install_modrinth_modpack");
        progress->setRange(0, 0);
        if (!quiet)
            message(tr("Выполняем действие…"));
        updatePlayState();
    }
    core->request(
        method, params,
        [this, done, method, mutation, quiet](const QJsonValue &result, const QJsonObject &error) {
            if (mutation) {
                busy = false;
                contentInstalling = false;
                if (error.isEmpty())
                    projectView->setInstalling(false);
                else
                    projectView->setInstallationError(value(error, "message"));
                progress->setRange(0, 100);
                progress->setValue(error.isEmpty() ? 100 : 0);
                updatePlayState();
            }
            if (!error.isEmpty()) {
                if (method == s("install_update"))
                    updateStatus->setText(value(error, "message"));
                message(value(error, "message"), true);
                return;
            }
            if (done)
                done(result);
            if (mutation && !quiet && operationId.isEmpty() &&
                !activity->property("error").toBool())
                message(tr("Готово"));
        });
}
void LauncherWindow::initialize() {
    ready = true;
    call(s("get_profile"), {}, [this](const QJsonValue &v) {
        profile = v.toObject();
        QSignalBlocker a(memory), b(memorySlider);
        memory->setValue(profile.value(s("memoryMb")).toInt(4096));
        memorySlider->setValue(memory->value());
        gameDirectory->setText(value(profile, "gameDir"));
    });
    refreshLibrary();
    refreshAccounts();
    refreshRuntimes();
    loadVersions();
    call(s("memory_status"), {}, [this](const QJsonValue &v) {
        auto m = v.toObject();
        QSignalBlocker a(memory), b(memorySlider);
        int low = m.value(s("minMemoryMb")).toInt(1024),
            high = m.value(s("maxMemoryMb")).toInt(8192),
            step = m.value(s("stepMemoryMb")).toInt(512);
        memory->setRange(low, high);
        memory->setSingleStep(step);
        memorySlider->setRange(low, high);
        memorySlider->setSingleStep(step);
        memorySlider->setValue(memory->value());
    });
    updatePlayState();
}
void LauncherWindow::updatePlayState() {
    if (projectView) {
        projectView->setInstalling(contentInstalling);
        projectView->setActionBlockedReason(
            !ready                   ? tr("Игровое ядро недоступно. Перезапустите лаунчер.")
            : running                ? tr("Остановите Minecraft перед установкой контента.")
            : !operationId.isEmpty() ? tr("Дождитесь запуска Minecraft или отмените его.")
            : busy && !contentInstalling
                ? tr("Дождитесь завершения текущей операции или остановите её.")
                : QString());
    }
    for (auto *b : findChildren<QPushButton *>()) {
        if (b->property("playAction").toBool())
            b->setEnabled(ready && !busy && !running && operationId.isEmpty());
    }
    if (play)
        play->setText(running                  ? tr("Игра запущена")
                      : !operationId.isEmpty() ? tr("Запускаем…")
                                               : tr("Играть"));
}
void LauncherWindow::cancel() {
    if (running && !operationId.isEmpty())
        core->request(s("stop_game"), {{s("operationId"), operationId}});
    else {
        core->request(s("cancel_content_operation"));
        if (!operationId.isEmpty())
            core->request(s("cancel_operation"), {{s("operationId"), operationId}});
    }
    message(tr("Останавливаем операцию…"));
}
void LauncherWindow::resizeEvent(QResizeEvent *e) {
    QMainWindow::resizeEvent(e);
    if (activity)
        activity->move(width() - activity->width() - 28, 65);
}
void LauncherWindow::closeEvent(QCloseEvent *event) {
    if (running || busy || !operationId.isEmpty()) {
        if (QMessageBox::question(
                this, tr("Закрыть лаунчер?"), tr("Сначала остановить игру или текущую загрузку?"),
                QMessageBox::Yes | QMessageBox::No, QMessageBox::No) == QMessageBox::Yes)
            cancel();
        event->ignore();
        return;
    }
    closing = true;
    QSettings settings;
    settings.setValue(s("geometry"), saveGeometry());
    core->shutdown();
    event->accept();
}
#ifdef Q_OS_WIN
#if QT_VERSION >= QT_VERSION_CHECK(6, 0, 0)
bool LauncherWindow::nativeEvent(const QByteArray &type, void *data, qintptr *result) {
#else
bool LauncherWindow::nativeEvent(const QByteArray &type, void *data, long *result) {
#endif
    auto *msg = static_cast<MSG *>(data);
    if (msg->message == WM_NCHITTEST && !isMaximized()) {
        RECT bounds{};
        if (GetWindowRect(msg->hwnd, &bounds)) {
            const int x = short(LOWORD(msg->lParam)) - bounds.left,
                      y = short(HIWORD(msg->lParam)) - bounds.top;
            const int width = bounds.right - bounds.left, height = bounds.bottom - bounds.top,
                      border = qRound(6 * devicePixelRatioF());
            if (x >= 0 && y >= 0 && x < width && y < height) {
                const bool left = x < border, right = x >= width - border, top = y < border,
                           bottom = y >= height - border;
                if (left || right || top || bottom) {
                    *result = top      ? (left    ? HTTOPLEFT
                                          : right ? HTTOPRIGHT
                                                  : HTTOP)
                              : bottom ? (left    ? HTBOTTOMLEFT
                                          : right ? HTBOTTOMRIGHT
                                                  : HTBOTTOM)
                              : left   ? HTLEFT
                                       : HTRIGHT;
                    return true;
                }
            }
        }
    }
    if (msg->message == WM_GETMINMAXINFO) {
        auto *info = reinterpret_cast<MINMAXINFO *>(msg->lParam);
        MONITORINFO monitor{};
        monitor.cbSize = sizeof(monitor);
        if (GetMonitorInfoW(MonitorFromWindow(msg->hwnd, MONITOR_DEFAULTTONEAREST), &monitor)) {
            info->ptMaxPosition.x = monitor.rcWork.left - monitor.rcMonitor.left;
            info->ptMaxPosition.y = monitor.rcWork.top - monitor.rcMonitor.top;
            info->ptMaxSize.x = monitor.rcWork.right - monitor.rcWork.left;
            info->ptMaxSize.y = monitor.rcWork.bottom - monitor.rcWork.top;
            info->ptMinTrackSize.x = qRound(minimumWidth() * devicePixelRatioF());
            info->ptMinTrackSize.y = qRound(minimumHeight() * devicePixelRatioF());
            *result = 0;
            return true;
        }
    }
    return QMainWindow::nativeEvent(type, data, result);
}
#endif
