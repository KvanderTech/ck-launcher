#include "window.h"
#include "widgets.h"
#include <QJsonDocument>
#include <QSettings>
LauncherWindow::LauncherWindow(Backend *backend, QWidget *parent)
    : QMainWindow(parent), core(backend) {
    setWindowTitle(tr("ЦК · Minecraft Launcher"));
    setWindowIcon(QIcon(s(":/assets/logo.png")));
    resize(1160, 800);
    setMinimumSize(850, 620);
    auto *central = new QWidget;
    setCentralWidget(central);
    auto *vertical = new QVBoxLayout(central);
    vertical->setContentsMargins(0, 0, 0, 0);
    vertical->setSpacing(0);
    auto *body = new QHBoxLayout;
    body->setSpacing(0);
    vertical->addLayout(body, 1);
    auto *sidebar = new QWidget;
    sidebar->setObjectName(s("sidebar"));
    sidebar->setFixedWidth(196);
    auto *side = new QVBoxLayout(sidebar);
    side->setContentsMargins(18, 24, 18, 18);
    side->setSpacing(20);
    auto *brand = new QLabel;
    brand->setPixmap(QPixmap(s(":/assets/logo.png"))
                         .scaled(66, 66, Qt::KeepAspectRatio, Qt::SmoothTransformation));
    side->addWidget(brand);
    auto *title = new QLabel(tr("ЦК LAUNCHER"));
    title->setProperty("brand", true);
    side->addWidget(title);
    navigation = new QListWidget;
    navigation->setObjectName(s("navigation"));
    navigation->addItems(
        {tr("Мои сборки"), tr("Каталог"), tr("Аккаунты и скины"), tr("Настройки"), tr("Журналы")});
    side->addWidget(navigation, 1);
    accountLabel = new QLabel(tr("Подключение…"));
    accountLabel->setWordWrap(true);
    side->addWidget(accountLabel);
    auto *version = new QLabel(tr("Qt Widgets · %1").arg(s(CK_VERSION)));
    version->setProperty("muted", true);
    version->setWordWrap(true);
    side->addWidget(version);
    body->addWidget(sidebar);
    pages = new QStackedWidget;
    body->addWidget(pages, 1);
    pages->addWidget(libraryPage());
    pages->addWidget(catalogPage());
    pages->addWidget(accountsPage());
    pages->addWidget(settingsPage());
    pages->addWidget(logsPage());
    auto *footer = new QWidget;
    footer->setObjectName(s("footer"));
    auto *foot = new QVBoxLayout(footer);
    foot->setContentsMargins(20, 10, 20, 10);
    status = new QLabel(tr("Запускаем игровое ядро…"));
    status->setWordWrap(true);
    status->setTextFormat(Qt::PlainText);
    foot->addWidget(status);
    progress = new QProgressBar;
    progress->setRange(0, 100);
    progress->setValue(0);
    progress->setFixedHeight(6);
    progress->setTextVisible(false);
    foot->addWidget(progress);
    vertical->addWidget(footer);
    connect(navigation, &QListWidget::currentRowChanged, this, [this](int row) {
        pages->setCurrentIndex(row);
        if (row == 1 && catalog.isEmpty())
            searchCatalog();
        if (row == 4)
            showLogs();
    });
    navigation->setCurrentRow(0);
    connect(core, &Backend::ready, this, &LauncherWindow::initialize);
    connect(core, &Backend::disconnected, this, [this](const QString &reason) {
        if (!closing) {
            message(reason, true);
            play->setEnabled(false);
        }
    });
    connect(core, &Backend::event, this, [this](const QString &event, const QJsonValue &payload) {
        auto data = payload.toObject();
        if (event == s("launcher://progress")) {
            const auto total = data.value(s("totalBytes")).toDouble();
            const auto done = data.value(s("completedBytes")).toDouble();
            progress->setRange(0, total > 0 ? 100 : 0);
            if (total > 0)
                progress->setValue(qBound(0, int(done * 100 / total), 100));
            auto stage = value(data, "stage");
            if (!stage.isEmpty())
                message(tr("Подготовка игры: %1").arg(stage));
        } else if (event == s("launcher://game-started")) {
            running = true;
            operationId = value(data, "operationId");
            play->setEnabled(false);
            stop->setEnabled(true);
            progress->setRange(0, 100);
            progress->setValue(100);
            message(tr("Minecraft запущен. Хорошей игры!"));
        } else if (event == s("launcher://game-exited")) {
            running = false;
            operationId.clear();
            play->setEnabled(true);
            stop->setEnabled(false);
            message(tr("Игра завершена. Сборка готова к следующему запуску."));
        } else if (event == s("launcher://error")) {
            running = false;
            operationId.clear();
            play->setEnabled(true);
            stop->setEnabled(false);
            progress->setRange(0, 100);
            message(value(data.value(s("error")).toObject(), "message"), true);
        }
    });
    QSettings settings;
    if (settings.contains(s("geometry")))
        restoreGeometry(settings.value(s("geometry")).toByteArray());
}
void LauncherWindow::message(const QString &text, bool error) {
    status->setText(text);
    status->setStyleSheet(error ? s("color:#ffb2a8;") : s("color:#b4c7ce;"));
}
void LauncherWindow::call(const QString &method, const QJsonObject &params,
                          std::function<void(const QJsonValue &)> done, bool mutation) {
    if (mutation && busy) {
        message(tr("Дождитесь завершения текущей операции."), true);
        return;
    }
    if (mutation) {
        busy = true;
        progress->setRange(0, 0);
        message(tr("Выполняется операция…"));
    }
    core->request(method, params,
                  [this, done, mutation](const QJsonValue &result, const QJsonObject &error) {
                      if (mutation) {
                          busy = false;
                          progress->setRange(0, 100);
                          progress->setValue(error.isEmpty() ? 100 : 0);
                      }
                      if (!error.isEmpty()) {
                          message(value(error, "message"), true);
                          return;
                      }
                      if (mutation)
                          message(tr("Готово."));
                      if (done)
                          done(result);
                  });
}
void LauncherWindow::initialize() {
    play->setEnabled(true);
    message(tr("Готов к работе. Локальные сборки доступны без каталога."));
    call(s("get_profile"), {}, [this](const QJsonValue &v) {
        profile = v.toObject();
        memory->setValue(profile.value(s("memoryMb")).toInt(4096));
    });
    refreshLibrary();
    refreshAccounts();
    refreshRuntimes();
    loadVersions();
    call(s("memory_status"), {}, [this](const QJsonValue &v) {
        auto x = v.toObject();
        memory->setRange(x.value(s("minMemoryMb")).toInt(1024),
                         x.value(s("maxMemoryMb")).toInt(8192));
        memory->setSingleStep(x.value(s("stepMemoryMb")).toInt(512));
    });
}
void LauncherWindow::closeEvent(QCloseEvent *event) {
    if (running || busy || !operationId.isEmpty()) {
        QMessageBox::information(
            this, tr("Операция ещё выполняется"),
            tr("Остановите игру или отмените загрузку перед закрытием лаунчера."));
        event->ignore();
        return;
    }
    closing = true;
    QSettings settings;
    settings.setValue(s("geometry"), saveGeometry());
    core->shutdown();
    event->accept();
}
