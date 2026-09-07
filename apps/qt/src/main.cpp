#include "widgets.h"
#include "window.h"
#include <QCryptographicHash>
#include <QJsonDocument>
#include <QLocalServer>
#include <QLocalSocket>
#include <QTemporaryDir>
int main(int argc, char **argv) {
#if QT_VERSION < QT_VERSION_CHECK(6, 0, 0)
    QCoreApplication::setAttribute(Qt::AA_EnableHighDpiScaling);
    QCoreApplication::setAttribute(Qt::AA_UseHighDpiPixmaps);
#endif
    QApplication app(argc, argv);
    app.setOrganizationName(s("CKLauncher"));
    app.setApplicationName(s("CKLauncherQt"));
    app.setApplicationVersion(s(CK_VERSION));
    app.setStyle(s("Fusion"));
    const auto args = app.arguments();
    const bool smoke = args.contains(s("--smoke"));
    QTemporaryDir smokeData;
    if (smoke) {
        app.setApplicationName(s("CKLauncherQtSmoke"));
        QFontDatabase::addApplicationFont(qEnvironmentVariable("WINDIR") + s("/Fonts/segoeui.ttf"));
    }
    QString pack;
    for (const auto &arg : args.mid(1))
        if (arg.endsWith(s(".mrpack"), Qt::CaseInsensitive))
            pack = QFileInfo(arg).absoluteFilePath();
    const auto data = QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    QDir().mkpath(data);
    const auto serverName = s("CKLauncherQt-") +
                            QString::fromLatin1(QCryptographicHash::hash(QDir::homePath().toUtf8(),
                                                                         QCryptographicHash::Sha256)
                                                    .toHex()
                                                    .left(20));
    QLockFile lock(data + s("/instance.lock"));
    lock.setStaleLockTime(0);
    if (!smoke && !lock.tryLock(0)) {
        QLocalSocket socket;
        socket.connectToServer(serverName);
        if (socket.waitForConnected(800)) {
            socket.write(
                QJsonDocument(QJsonObject{{s("path"), pack}}).toJson(QJsonDocument::Compact) +
                '\n');
            socket.waitForBytesWritten(800);
            return 0;
        }
        QMessageBox::warning(nullptr, QObject::tr("Лаунчер уже открыт"),
                             QObject::tr("Переключитесь на существующее окно лаунчера."));
        return 1;
    }
    QFile theme(s(":/assets/theme.qss"));
    if (theme.open(QIODevice::ReadOnly))
        app.setStyleSheet(QString::fromUtf8(theme.readAll()));
    Backend core;
    LauncherWindow window(&core);
    QLocalServer server;
    server.setSocketOptions(QLocalServer::UserAccessOption);
    if (!smoke) {
        QLocalServer::removeServer(serverName);
        server.listen(serverName);
    }
    QObject::connect(&server, &QLocalServer::newConnection, &window, [&] {
        while (auto *socket = server.nextPendingConnection()) {
            socket->setReadBufferSize(8192);
            auto bytes = std::make_shared<QByteArray>();
            QObject::connect(socket, &QLocalSocket::readyRead, &window, [&, socket, bytes] {
                *bytes += socket->readAll();
                if (bytes->size() > 4096) {
                    socket->disconnectFromServer();
                    return;
                }
                if (!bytes->contains('\n'))
                    return;
                auto message = QJsonDocument::fromJson(bytes->left(bytes->indexOf('\n'))).object();
                auto path = value(message, "path");
                window.showNormal();
                window.raise();
                window.activateWindow();
                if (!path.isEmpty() && QFileInfo(path).isAbsolute() &&
                    path.endsWith(s(".mrpack"), Qt::CaseInsensitive))
                    window.importPack(path);
                socket->disconnectFromServer();
            });
            QObject::connect(socket, &QLocalSocket::disconnected, socket, &QObject::deleteLater);
            QTimer::singleShot(3000, socket, [socket] { socket->disconnectFromServer(); });
        }
    });
    window.show();
    auto environment = QProcessEnvironment::systemEnvironment();
    if (smoke) {
        environment.insert(s("APPDATA"), smokeData.path());
        QObject::connect(&core, &Backend::ready, &app, [&] {
            core.request(s("create_build"),
                         {{s("name"), QObject::tr("Проверка Qt · 1.20.1")},
                          {s("gameVersion"), s("1.20.1")},
                          {s("loader"), s("vanilla")}},
                         [&](const QJsonValue &, const QJsonObject &error) {
                             if (!error.isEmpty()) {
                                 app.exit(2);
                                 return;
                             }
                             window.initialize();
                             QTimer::singleShot(1500, &app, [&] {
                                 const auto index = args.indexOf(s("--screenshot"));
                                 if (index >= 0 && index + 1 < args.size())
                                     window.grab().save(args[index + 1]);
                                 core.shutdown();
                                 app.exit(0);
                             });
                         });
        });
        QTimer::singleShot(20000, &app, [&] { app.exit(3); });
    } else if (!pack.isEmpty())
        QObject::connect(&core, &Backend::ready, &window,
                         [&window, pack] { window.importPack(pack); });
    const int serviceOption = args.indexOf(s("--service"));
    const auto executable =
        serviceOption >= 0 && serviceOption + 1 < args.size()
            ? args[serviceOption + 1]
            : QCoreApplication::applicationDirPath() + s("/ck-launcher-service.exe");
    core.start(executable, environment);
    QObject::connect(&app, &QCoreApplication::aboutToQuit, &core, &Backend::shutdown);
    return app.exec();
}
